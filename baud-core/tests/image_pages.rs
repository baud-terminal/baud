//! **Freed image pages go back, and nothing live goes with them**
//! (`Term::set_page_release`, `baud_domain::PageRelease`). The release
//! lent here is a test double: 64-byte pages, so small images share
//! pages, and every byte it is handed is overwritten with `0xA5`, a
//! pattern no image below writes -- the worst a system's release could
//! leave, since what a released page holds is unspecified. A page handed
//! over that a live image still used, or a re-stored image that left a
//! byte of its span unwritten, then shows as `0xA5A5A5A5` where a colour
//! was. The real release, and the resident size it gives back, are
//! baud-app's (`pages.rs`).

use baud_core::*;
use baud_domain::{
    Chan, DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KittyId, PageRelease, PagesBack,
    Pixel,
};
use std::cell::Cell;
use std::io::Write as _;

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// The double's page, in bytes.
const PAGE: usize = 64;

/// What the double writes over every byte it is handed.
const POISON: u8 = 0xa5;

thread_local! {
    /// How many spans the double was handed on this thread.
    static CALLS: Cell<u64> = const { Cell::new(0) };
}

fn poison(span: &mut [u8]) -> PagesBack {
    span.fill(POISON);
    CALLS.with(|c| c.set(c.get() + 1));
    PagesBack::Released
}

fn release() -> PageRelease {
    PageRelease::new(PAGE, poison).expect("a power of two")
}

/// A terminal with `budget` and a 10x20 cell, the double lent or not.
fn term(owned: &mut Owned, lend: Lend) -> Term<'_> {
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    if lend == Lend::Yes {
        t.set_page_release(release());
    }
    t
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lend {
    Yes,
    No,
}

/// Feed `bytes` in reads of `READ_CHUNK`, dropping the replies, and
/// finish any deferred decode.
fn feed(t: &mut Term<'_>, bytes: &[u8]) {
    for c in bytes.chunks(READ_CHUNK) {
        t.write_at(BytePos::ZERO, c);
        t.feed(BytePos::ZERO, ReadLen::from_index(c.len()));
        t.clear_reply();
    }
    while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
    t.clear_reply();
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// RFC 4648 base64, padded.
fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            let six = usize::try_from((n >> (18 - 6 * k)) & 63).unwrap();
            out.push(if k <= q.len() { B64[six] } else { b'=' });
        }
    }
    out
}

/// One graphics command, its payload base64'd.
fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend(b64(payload));
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

/// RGBA bytes of a `w` x `h` image whose every pixel is opaque, depends on
/// `seed` and where it is, and is never the poison's word.
fn rgba(w: ImagePx, h: ImagePx, seed: Chan) -> Vec<u8> {
    let s = seed.get();
    let mut v = Vec::new();
    for y in 0..h.raw() {
        for x in 0..w.raw() {
            let (xl, yl) = (x.to_le_bytes()[0], y.to_le_bytes()[0]);
            v.extend_from_slice(&[s ^ xl, s.wrapping_add(yl), s | 1, 0xff]);
        }
    }
    v
}

/// RGBA bytes as the straight ARGB words the store keeps.
fn words(data: &[u8]) -> Vec<u32> {
    let c = Chan::from_u8;
    data.chunks(4)
        .map(|p| Pixel::straight(c(p[3]), c(p[0]), c(p[1]), c(p[2])).bits())
        .collect()
}

/// The words kitty image `id` holds.
fn shown(t: &Term<'_>, id: KittyId) -> Vec<u32> {
    let image = t.images().kitty_image(id).expect("stored");
    let v = t.images().image(image).expect("shown");
    v.pixels.iter().map(|p| p.bits()).collect()
}

/// Where kitty image `id`'s pixels lie in memory.
fn bounds(t: &Term<'_>, id: KittyId) -> core::ops::Range<*const Pixel> {
    let image = t.images().kitty_image(id).expect("stored");
    t.images()
        .image(image)
        .expect("shown")
        .pixels
        .as_ptr_range()
}

/// **A freed span's whole pages go back, its neighbour on a shared page
/// keeps every pixel, and an image stored over the poison reads back
/// exactly.** A is 37x5 (740 bytes), B 16x4 right after it, starting
/// inside A's last page.
#[test]
fn a_freed_span_goes_back_without_its_neighbours_page() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = term(&mut owned, Lend::Yes);
    let (a, b, c) = (
        rgba(ImagePx::of(37), ImagePx::of(5), Chan::from_u8(0x31)),
        rgba(ImagePx::of(16), ImagePx::of(4), Chan::from_u8(0x5c)),
        rgba(ImagePx::of(37), ImagePx::of(5), Chan::from_u8(0x9e)),
    );
    feed(&mut t, &apc("a=T,f=32,s=37,v=5,i=1", &a));
    feed(&mut t, &apc("a=T,f=32,s=16,v=4,i=2", &b));
    let (ra, rb) = (bounds(&t, KittyId::of(1)), bounds(&t, KittyId::of(2)));
    assert_eq!(ra.end, rb.start, "B lies right after A");
    let lo = ra.start.cast::<u8>().align_offset(PAGE);
    let whole = (a.len() - lo) / PAGE * PAGE;
    // **The control**: A holds whole pages, and B starts inside one.
    assert!(whole >= 8 * PAGE, "A holds whole pages: {whole}");
    assert_ne!(rb.start.cast::<u8>().align_offset(PAGE), 0, "mid-page");
    let before = CALLS.with(Cell::get);
    feed(&mut t, &apc("a=d,d=I,i=1", &[]));
    assert_eq!(shown(&t, KittyId::of(2)), words(&b), "B is pixel-exact");
    assert_eq!(
        CALLS.with(Cell::get),
        before + 1,
        "one release for one free"
    );
    assert_eq!(t.images().bytes_released, whole, "A's whole pages, no more");
    feed(&mut t, &apc("a=T,f=32,s=37,v=5,i=3", &c));
    assert_eq!(
        bounds(&t, KittyId::of(3)).start,
        ra.start,
        "C took A's span"
    );
    assert_eq!(shown(&t, KittyId::of(3)), words(&c), "C reads back exactly");
    assert_eq!(shown(&t, KittyId::of(2)), words(&b), "B still exact");
}

/// **An image a placement still names keeps its pages**: an uppercase
/// delete of one of two placements frees no image, so nothing goes back.
#[test]
fn an_image_still_placed_keeps_its_pages() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = term(&mut owned, Lend::Yes);
    let a = rgba(ImagePx::of(64), ImagePx::of(4), Chan::from_u8(0x66));
    feed(&mut t, &apc("a=T,f=32,s=64,v=4,i=4,p=1", &a));
    feed(&mut t, &apc("a=p,i=4,p=2", &[]));
    feed(&mut t, &apc("a=d,d=I,i=4,p=1", &[]));
    assert_eq!(t.images().placements().count(), 1, "one placement left");
    assert_eq!(shown(&t, KittyId::of(4)), words(&a), "its pixels are whole");
    assert_eq!(t.images().bytes_released, 0, "no image was freed");
}

/// **Every way an image is freed gives its pages back**: a delete, the
/// delete of an image only a virtual placement held, an eviction for
/// space, `RIS`, `ED 2`, and its screen being left. Each
/// starts from a placed 64x4 image (1024 bytes, at least fifteen whole
/// double pages) and counts what went back.
#[test]
fn every_free_gives_its_pages_back() {
    let a = rgba(ImagePx::of(64), ImagePx::of(4), Chan::from_u8(0x12));
    let placed = apc("a=T,f=32,s=64,v=4,i=1", &a);
    let unnamed = apc("a=T,f=32,s=64,v=4", &a);
    let evicting = {
        // Two unplaced images fill a 512-pixel arena; a third evicts the
        // oldest.
        let mut s = apc("a=t,f=32,s=64,v=4,i=5", &a);
        s.extend(apc("a=t,f=32,s=64,v=4,i=6", &a));
        s.extend(apc("a=t,f=32,s=64,v=4,i=7", &a));
        s
    };
    let mut virtual_only = apc("a=t,f=32,s=64,v=4,i=1", &a);
    virtual_only.extend(apc("a=p,U=1,i=1,c=4,r=3", b""));
    let cases: [(&str, &[u8], &[u8], usize); 6] = [
        ("delete", &placed, b"\x1b_Ga=d,d=I,i=1\x1b\\", 4096),
        // Held by a virtual placement alone: its last reference let go.
        (
            "a virtual placement's delete",
            &virtual_only,
            b"\x1b_Ga=d,d=I,i=1\x1b\\",
            4096,
        ),
        ("eviction", b"", &evicting, 512),
        ("RIS", &placed, b"\x1bc", 4096),
        ("ED 2", &placed, b"\x1b[2J", 4096),
        ("leaving the alternate screen", b"", b"", 4096),
    ];
    for (what, setup, act, arena) in cases {
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(arena));
        let mut t = term(&mut owned, Lend::Yes);
        let (setup, act): (Vec<u8>, Vec<u8>) = match what {
            "leaving the alternate screen" => {
                let mut s = b"\x1b[?1049h".to_vec();
                s.extend(&unnamed);
                (s, b"\x1b[?1049l".to_vec())
            }
            _ => (setup.to_vec(), act.to_vec()),
        };
        feed(&mut t, &setup);
        assert_eq!(t.images().bytes_released, 0, "{what}: nothing freed yet");
        feed(&mut t, &act);
        // `f=32` writes no scratch, so what went back is a freed span.
        assert!(
            t.images().bytes_released >= 15 * PAGE,
            "{what}: its pages went back: {}",
            t.images().bytes_released
        );
    }
}

/// With no release lent nothing is handed and nothing counted.
#[test]
fn with_no_release_lent_nothing_is_handed() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = term(&mut owned, Lend::No);
    let before = CALLS.with(Cell::get);
    feed(
        &mut t,
        &apc(
            "a=T,f=32,s=64,v=4,i=1",
            &rgba(ImagePx::of(64), ImagePx::of(4), Chan::from_u8(7)),
        ),
    );
    feed(&mut t, b"\x1bc");
    assert_eq!(
        (CALLS.with(Cell::get), t.images().bytes_released),
        (before, 0)
    );
}

/// An 8-bit RGBA PNG of `data`, written by the `png` crate, its stream
/// cut into `IDAT` chunks of 97 bytes.
fn png(w: ImagePx, h: ImagePx, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w.raw().into(), h.raw().into());
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        let mut sw = wr.stream_writer_with_size(97).unwrap();
        sw.write_all(data).unwrap();
        sw.finish().unwrap();
    }
    out
}

/// A zlib stream of `data`, written by `miniz_oxide`.
fn zlib(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

/// **The scratch goes back once a PNG is done with it, and never while
/// one holds it.** Two PNGs, each sent whole, in `m=1` chunks, and with
/// `o=z`, are decoded one after another through the poisoned scratch: a
/// release while a PNG's bytes were waiting in it, or between its chunks,
/// would decode `0xA5` and fail or draw it. Each completed PNG hands its
/// scratch back once.
#[test]
fn the_png_scratch_goes_back_when_a_decode_is_done_and_not_before() {
    let (w, h) = (ImagePx::of(40), ImagePx::of(30));
    for (k, seed) in [0x21u8, 0x7d, 0xc4].into_iter().enumerate() {
        let data = rgba(w, h, Chan::from_u8(seed));
        let file = png(w, h, &data);
        let enc = b64(&file);
        let sends: Vec<Vec<u8>> = vec![
            apc("a=T,f=100,i=1", &file),
            {
                let mut v = Vec::new();
                let pieces: Vec<&[u8]> = enc.chunks(256).collect();
                for (n, p) in pieces.iter().enumerate() {
                    let more = if n + 1 == pieces.len() { 0 } else { 1 };
                    let keys = if n == 0 { "a=T,f=100,i=1," } else { "" };
                    v.extend_from_slice(format!("\x1b_G{keys}m={more};").as_bytes());
                    v.extend_from_slice(p);
                    v.extend_from_slice(b"\x1b\\");
                }
                v
            },
            {
                let z = zlib(&file);
                apc(&format!("a=T,f=100,o=z,S={},i=1", file.len()), &z)
            },
        ];
        for (how, send) in ["whole", "chunked", "o=z"].iter().zip(&sends) {
            let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
            let mut t = term(&mut owned, Lend::Yes);
            // A first picture through the scratch, decoded and deleted, so
            // the second meets a poisoned scratch and a poisoned span.
            let first = rgba(w, h, Chan::from_u8(seed ^ 0x55));
            feed(&mut t, &apc("a=T,f=100,i=9", &png(w, h, &first)));
            assert_eq!(shown(&t, KittyId::of(9)), words(&first), "{k} {how}: first");
            let handed = CALLS.with(Cell::get);
            assert!(
                t.images().bytes_released > 0,
                "{k} {how}: its scratch went back"
            );
            feed(&mut t, &apc("a=d,d=I,i=9", &[]));
            assert_eq!(
                CALLS.with(Cell::get),
                handed + 1,
                "{k} {how}: and its span, once"
            );
            feed(&mut t, send);
            assert_eq!(shown(&t, KittyId::of(1)), words(&data), "{k} {how}: exact");
            assert_eq!(
                CALLS.with(Cell::get),
                handed + 2,
                "{k} {how}: the scratch, once more"
            );
        }
    }
}

/// Ten bands of a Sixel `w` wide in `rgb`, with no terminator.
fn ten_bands(w: &str, rgb: &str) -> Vec<u8> {
    let mut v = format!("\x1bP9;1q#1;2;{rgb}#1").into_bytes();
    for band in 0..10 {
        if band > 0 {
            v.push(b'-');
        }
        v.extend(format!("!{w}~").into_bytes());
    }
    v
}

/// **A Sixel's written rows are never handed over**: its claim lies in a
/// gap the slot table cannot see, which the store keeps a record of. The
/// scenario of `sixel_stream.rs`'s
/// `a_resize_inside_a_body_frees_room_the_rest_of_it_can_use` -- an image
/// expired by a resize between two reads of a Sixel body, its span released
/// while the Sixel paints the gap beside it and may then move into it -- is
/// run at every cut with the double lent and without, and every image drawn
/// is the same in both.
#[test]
fn a_sixel_claim_survives_a_release_beside_it_at_every_cut() {
    let mut first = ten_bands("20", "0;0;100");
    first.extend_from_slice(b"\x1b\\");
    let body = ten_bands("30", "100;0;0");
    let ws = WinSize {
        ws_row: 12,
        ws_col: 20,
    };
    let run = |cut: core::ops::RangeTo<usize>, rest: core::ops::RangeFrom<usize>, lend| {
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(3000));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        if lend == Lend::Yes {
            t.set_page_release(release());
        }
        feed(&mut t, b"\x1b[?1049h\x1b[1;1H");
        feed(&mut t, &first);
        feed(&mut t, b"\x1b[6;1H");
        feed(&mut t, &first);
        feed(&mut t, b"\x1b[1;1H");
        feed(&mut t, &body[cut]);
        let _ = t.resize(&WinSize {
            ws_row: 5,
            ws_col: 20,
        });
        feed(&mut t, &body[rest]);
        feed(&mut t, b"\x1b\\");
        let im = t.images();
        let drawn: Vec<(LineIdx, Vec<u32>)> = im
            .placements()
            .map(|(_, p)| {
                let v = im.image(p.image).expect("placed means live");
                (p.line, v.pixels.iter().map(|c| c.bits()).collect())
            })
            .collect();
        (drawn, im.bytes_released)
    };
    let mut released = 0;
    for cut in 0..=body.len() {
        let (plain, none) = run(..cut, cut.., Lend::No);
        let (lent, bytes) = run(..cut, cut.., Lend::Yes);
        assert_eq!(none, 0);
        assert_eq!(plain, lent, "cut {cut}: the same images, pixel for pixel");
        released += bytes;
    }
    // **The control**: the resize freed an image, and its pages went back.
    assert!(released > 0, "the expired image's pages went back");
}

use baud_domain::LineIdx;

thread_local! {
    /// Every range the recording double was handed on this thread.
    static HANDED: std::cell::RefCell<Vec<(usize, usize)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The recording double: poisons what it is handed, as [`poison`] does,
/// and keeps its address and length.
fn record(span: &mut [u8]) -> PagesBack {
    HANDED.with(|h| h.borrow_mut().push((span.as_ptr().addr(), span.len())));
    span.fill(POISON);
    PagesBack::Released
}

/// The pages handed since the last call, by address, in the order
/// handed, a page handed twice twice.
fn handed_raw() -> Vec<usize> {
    let mut pages = Vec::new();
    for (at, len) in HANDED.with(|h| core::mem::take(&mut *h.borrow_mut())) {
        assert_eq!((at % PAGE, len % PAGE), (0, 0), "whole pages");
        pages.extend((at..at + len).step_by(PAGE));
    }
    pages
}

/// The pages handed since the last call, by address, each once: a page
/// handed twice fails here.
fn handed_pages() -> Vec<usize> {
    let pages = handed_raw();
    let mut once = pages.clone();
    once.sort_unstable();
    once.dedup();
    assert_eq!(once.len(), pages.len(), "a page handed twice: {pages:x?}");
    once
}

/// The pages from `lo`'s page boundary at or above to `hi`'s at or above.
fn pages_between(lo: usize, hi: usize) -> Vec<usize> {
    (lo.next_multiple_of(PAGE)..hi.next_multiple_of(PAGE))
        .step_by(PAGE)
        .collect()
}

/// A terminal over a `pixels` arena with the recording double lent.
fn recorded(owned: &mut Owned) -> Term<'_> {
    HANDED.with(|h| h.borrow_mut().clear());
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    t.set_page_release(PageRelease::new(PAGE, record).expect("a power of two"));
    t
}

/// **The page two neighbours share goes back with the second of them.**
/// A is 37x5 and B 16x4 right after it, starting inside A's last page.
/// A freed hands A's whole pages and not the shared one; B freed then
/// hands the shared page and B's own, through the page B's end is on,
/// which nothing else touches: every page the two touched, each once.
#[test]
fn the_page_two_freed_spans_share_goes_back_with_the_second() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = recorded(&mut owned);
    let (a, b, c) = (
        rgba(ImagePx::of(37), ImagePx::of(5), Chan::from_u8(0x31)),
        rgba(ImagePx::of(16), ImagePx::of(4), Chan::from_u8(0x5c)),
        rgba(ImagePx::of(60), ImagePx::of(5), Chan::from_u8(0x9e)),
    );
    feed(&mut t, &apc("a=T,f=32,s=37,v=5,i=1", &a));
    feed(&mut t, &apc("a=T,f=32,s=16,v=4,i=2", &b));
    let (ra, rb) = (bounds(&t, KittyId::of(1)), bounds(&t, KittyId::of(2)));
    let (a0, b0, b1) = (ra.start.addr(), rb.start.addr(), rb.end.addr());
    // **The control**: B starts inside a page A also holds.
    assert_eq!(ra.end, rb.start, "B lies right after A");
    assert_ne!(b0 % PAGE, 0, "mid-page");
    feed(&mut t, &apc("a=d,d=I,i=1", &[]));
    let first = handed_pages();
    assert_eq!(first, pages_between(a0, b0 - PAGE + 1), "A's own pages");
    assert_eq!(shown(&t, KittyId::of(2)), words(&b), "B is pixel-exact");
    feed(&mut t, &apc("a=d,d=I,i=2", &[]));
    let second = handed_pages();
    assert_eq!(
        second,
        pages_between(b0 - PAGE + 1, b1),
        "the shared page and B's, through B's last"
    );
    feed(&mut t, &apc("a=T,f=32,s=60,v=5,i=3", &c));
    assert_eq!(
        bounds(&t, KittyId::of(3)).start,
        ra.start,
        "C took A's span"
    );
    assert_eq!(shown(&t, KittyId::of(3)), words(&c), "C reads back exactly");
}

/// A Sixel `w` x `h` in green with its raster given, every pixel set,
/// terminated or not.
fn green_sixel(w: usize, h: usize, end: &[u8]) -> Vec<u8> {
    let mut v = format!("\x1bP9;1;0q\"1;1;{w};{h}#1;2;0;100;0#1").into_bytes();
    for band in 0..h.div_ceil(6) {
        if band > 0 {
            v.push(b'-');
        }
        v.extend(format!("!{w}~").into_bytes());
    }
    v.extend_from_slice(end);
    v
}

/// **A Sixel refused at its terminator gives its claim back**: every
/// image slot is held by a 1x1 picture, so a 40x60 Sixel decodes into
/// the gap past them and is refused for the slot. Its rows go back --
/// the pages touching them that no picture touches -- and a picture
/// stored there afterwards reads back exactly.
#[test]
fn a_refused_sixel_gives_its_claim_back() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = recorded(&mut owned);
    let dot = rgba(ImagePx::of(1), ImagePx::of(1), Chan::from_u8(0x44));
    for i in 1..=32 {
        feed(&mut t, &apc(&format!("a=t,f=32,s=1,v=1,i={i}"), &dot));
    }
    let lo = bounds(&t, KittyId::of(32)).end.addr();
    let refused = t.images().images_refused;
    feed(&mut t, &green_sixel(40, 60, b"\x1b\\"));
    // **The control**: refused for the slot, so it wrote and kept nothing.
    assert_eq!(t.images().images_refused, refused + 1, "refused");
    assert_eq!(
        handed_pages(),
        pages_between(lo, lo + 40 * 60 * 4),
        "its rows' pages"
    );
    feed(&mut t, &apc("a=d,d=I,i=1", &[]));
    handed_pages();
    let c = rgba(ImagePx::of(40), ImagePx::of(60), Chan::from_u8(0x9e));
    feed(&mut t, &apc("a=t,f=32,s=40,v=60,i=40", &c));
    assert_eq!(
        bounds(&t, KittyId::of(40)).start.addr(),
        lo,
        "over the claim"
    );
    assert_eq!(shown(&t, KittyId::of(40)), words(&c), "reads back exactly");
}

/// **A Sixel moved at its terminator gives back the rows it left**: a
/// 40x40 picture between two 1x1s is deleted, so a 20x30 Sixel claims
/// the larger gap past them and moves into the hole at its terminator.
/// What it wrote in the gap goes back, and the Sixel is whole.
#[test]
fn a_moved_sixel_gives_back_the_rows_it_left() {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = recorded(&mut owned);
    let dot = rgba(ImagePx::of(1), ImagePx::of(1), Chan::from_u8(0x44));
    let g = rgba(ImagePx::of(40), ImagePx::of(40), Chan::from_u8(0x21));
    feed(&mut t, &apc("a=t,f=32,s=1,v=1,i=1", &dot));
    feed(&mut t, &apc("a=t,f=32,s=40,v=40,i=2", &g));
    feed(&mut t, &apc("a=t,f=32,s=1,v=1,i=3", &dot));
    let hole = bounds(&t, KittyId::of(2)).start;
    let lo = bounds(&t, KittyId::of(3)).end.addr();
    feed(&mut t, &apc("a=d,d=I,i=2", &[]));
    handed_pages();
    feed(&mut t, &green_sixel(20, 30, b"\x1b\\"));
    let im = t.images();
    let (_, p) = im.placements().next().expect("the Sixel is placed");
    let v = im.image(p.image).expect("live");
    // **The control**: it moved into the hole.
    assert_eq!(v.pixels.as_ptr(), hole, "moved into the hole");
    let green = Pixel::opaque(Chan::from_u8(0), Chan::from_u8(255), Chan::from_u8(0));
    assert!(v.pixels.iter().all(|&p| p == green), "the Sixel is whole");
    assert_eq!(
        handed_pages(),
        pages_between(lo, lo + 20 * 30 * 4),
        "the rows it left"
    );
}

/// **A Sixel cut off by `CAN` keeps its claim as the image it placed**, and
/// gives it back when the image goes: a title or another Sixel after it
/// hands back none of its rows, and `RIS`, which removes every image, all
/// of them. The cut placed what it received; until then the claim was let
/// go at the next string.
#[test]
fn a_sixel_cut_off_keeps_its_claim_and_ris_gives_it_back() {
    let green = Pixel::opaque(Chan::from_u8(0), Chan::from_u8(255), Chan::from_u8(0));
    for (what, next, back) in [
        ("a title", &b"\x1b]2;x\x07"[..], false),
        ("another Sixel", &b"\x1bPq#0!1~\x1b\\"[..], false),
        ("RIS", &b"\x1bc"[..], true),
    ] {
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = recorded(&mut owned);
        feed(&mut t, &green_sixel(40, 60, b"\x18"));
        assert_eq!(handed_raw(), Vec::<usize>::new(), "{what}: not yet");
        {
            let im = t.images();
            let (_, p) = im.placements().next().expect("the cut-off Sixel is placed");
            let v = im.image(p.image).expect("live");
            assert_eq!((v.width.raw(), v.height.raw()), (40, 60), "{what}");
            assert!(v.pixels.iter().all(|&p| p == green), "{what}: whole");
        }
        feed(&mut t, next);
        let mut got = handed_raw();
        got.sort_unstable();
        got.dedup();
        match back {
            true => assert!(
                got.len() * PAGE >= 40 * 60 * 4 - 2 * PAGE,
                "{what}: its rows went back: {} pages",
                got.len()
            ),
            false => assert!(
                got.len() * PAGE < 40 * 60 * 4 - 2 * PAGE,
                "{what}: the image keeps its rows: {} pages back",
                got.len()
            ),
        }
    }
}

/// A release whose system call failed: it touches nothing, says so, and
/// counts its calls.
fn fail(_: &mut [u8]) -> PagesBack {
    CALLS.with(|c| c.set(c.get() + 1));
    PagesBack::Kept
}

/// A release that succeeded and left zero, as both systems' calls do.
fn zero(span: &mut [u8]) -> PagesBack {
    span.fill(0);
    PagesBack::Released
}

/// What a picture stored where a 64x16 one was deleted reads back, the
/// deleted one's pages handed to `release` or no release lent: the
/// decoders that write over a freed span, each from a fresh terminal.
fn over_a_freed_picture(release: Option<PageRelease>) -> Vec<Vec<u32>> {
    let old = rgba(ImagePx::of(64), ImagePx::of(16), Chan::from_u8(0x7b));
    let c = rgba(ImagePx::of(24), ImagePx::of(10), Chan::from_u8(0x3e));
    let rgb: Vec<u8> = c.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    // A Sixel whose first band is ten pixels wide and its second a
    // hundred: its stride, 64 after the first band, widens under the first
    // band's rows, and every bit but the top one of each band is unset.
    let sixel = b"\x1bP9;1;0q#1;2;100;0;0#1!10@-!100@\x1b\\".to_vec();
    let sends: [Vec<u8>; 4] = [
        sixel,
        apc("a=T,f=24,s=24,v=10,i=2", &rgb),
        apc("a=T,f=100,i=2", &png(ImagePx::of(24), ImagePx::of(10), &c)),
        apc("a=T,f=32,o=z,s=24,v=10,i=2", &zlib(&c)),
    ];
    let mut out = Vec::new();
    for send in sends {
        let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
        if let Some(r) = release {
            t.set_page_release(r);
        }
        feed(&mut t, &apc("a=T,f=32,s=64,v=16,i=1", &old));
        let at = bounds(&t, KittyId::of(1)).start;
        feed(&mut t, &apc("a=d,d=I,i=1", &[]));
        feed(&mut t, &send);
        let im = t.images();
        let (_, p) = im.placements().next().expect("placed");
        let v = im.image(p.image).expect("live");
        assert_eq!(v.pixels.as_ptr(), at, "stored over the deleted picture");
        out.push(v.pixels.iter().map(|c| c.bits()).collect());
    }
    out
}

/// **A release that fails leaves nothing drawn wrong, and counts
/// nothing.** Darwin's `mmap.2` and Linux's `madvise.2` say what a
/// successful call leaves and nothing of a failed one, so a failure may
/// leave the old picture where it was. A Sixel with unset pixels and a
/// widening stride, an RGB picture converted in place, a PNG and an
/// `o=z` picture are each stored over a deleted picture whose release
/// failed, and each reads back as it does where no release was lent, over
/// the poison and over zero: no decoder reads a word of a freed span
/// before writing it. The calls were made, and `bytes_released` stays
/// zero: it counts only pages the system took.
#[test]
fn a_release_that_fails_leaves_nothing_drawn_wrong() {
    let before = CALLS.with(Cell::get);
    let failed = over_a_freed_picture(PageRelease::new(PAGE, fail));
    assert!(CALLS.with(Cell::get) > before, "the release was asked");
    assert_eq!(failed, over_a_freed_picture(None), "as with no release");
    assert_eq!(
        failed,
        over_a_freed_picture(Some(release())),
        "as over poison"
    );
    assert_eq!(
        failed,
        over_a_freed_picture(PageRelease::new(PAGE, zero)),
        "as over zero"
    );
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(4096));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_page_release(PageRelease::new(PAGE, fail).expect("a power of two"));
    let old = rgba(ImagePx::of(64), ImagePx::of(16), Chan::from_u8(0x7b));
    feed(&mut t, &apc("a=T,f=32,s=64,v=16,i=1", &old));
    feed(&mut t, &apc("a=d,d=I,i=1", &[]));
    assert_eq!(t.images().bytes_released, 0, "nothing was taken");
    // **The control**: the Sixel's unset pixels are clear, so a stale
    // word would show; its set ones are red.
    let clear = Pixel::CLEAR.bits();
    assert!(failed[0].contains(&clear), "unset pixels are clear");
    assert!(
        failed[0].iter().any(|&p| p != clear),
        "and some are painted"
    );
}

/// **A picture freed beside a Sixel's claim keeps the claim's rows**: a
/// 37x6 Sixel on row 8 ends mid-page, a Sixel's claim starts there,
/// and a resize to five rows between two reads of its body expires the
/// picture. The page the two
/// share then holds the Sixel's first rows, and only the pages past what
/// the Sixel wrote may go back. Run at every cut with the poisoning
/// double and without; the Sixel drawn is the same.
#[test]
fn a_picture_freed_beside_a_claim_keeps_the_claims_rows() {
    let body = ten_bands("30", "100;0;0");
    let ws = WinSize {
        ws_row: 12,
        ws_col: 20,
    };
    // 37x6 pixels, 888 bytes: it ends 56 bytes into a page.
    let x = b"\x1bP9;1;0q\"1;1;37;6#1;2;0;0;100#1!37~\x1b\\";
    let run = |cut: usize, lend| {
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(3000));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(ImagePx::of(8), ImagePx::of(16));
        if lend == Lend::Yes {
            t.set_page_release(release());
        }
        feed(&mut t, b"\x1b[?1049h\x1b[8;1H");
        feed(&mut t, x);
        assert_eq!(t.images().placements().count(), 1, "the picture is placed");
        feed(&mut t, b"\x1b[1;1H");
        feed(&mut t, &body[..cut]);
        let _ = t.resize(&WinSize {
            ws_row: 5,
            ws_col: 20,
        });
        feed(&mut t, &body[cut..]);
        feed(&mut t, b"\x1b\\");
        let im = t.images();
        let gone = im.placements().count() == 1;
        let drawn: Vec<Vec<u32>> = im
            .placements()
            .map(|(_, p)| {
                let v = im.image(p.image).expect("placed means live");
                v.pixels.iter().map(|c| c.bits()).collect()
            })
            .collect();
        (drawn, gone, im.bytes_released)
    };
    let mut released = 0;
    for cut in 0..=body.len() {
        let (plain, gone, _) = run(cut, Lend::No);
        let (lent, _, bytes) = run(cut, Lend::Yes);
        assert!(gone, "cut {cut}: the control, the picture expired");
        assert_eq!(plain.len(), 1, "cut {cut}: the Sixel is drawn");
        assert_eq!(plain, lent, "cut {cut}: the same Sixel, pixel for pixel");
        released += bytes;
    }
    assert!(released > 0, "the expired picture's pages went back");
}
