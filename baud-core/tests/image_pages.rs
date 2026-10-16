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
    Chan, DecodeBudget, DecodeProgress, ImageBudget, ImagePx, KittyId, PageRelease, Pixel,
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

fn poison(span: &mut [u8]) {
    span.fill(POISON);
    CALLS.with(|c| c.set(c.get() + 1));
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

/// **Every way an image is freed gives its pages back**: a delete, an
/// eviction for space, `RIS`, `ED 2`, and its screen being left. Each
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
    let cases: [(&str, &[u8], &[u8], usize); 5] = [
        ("delete", &placed, b"\x1b_Ga=d,d=I,i=1\x1b\\", 4096),
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

/// **A Sixel's claim is never handed over**: it lies in a gap the slot
/// table cannot see, and only a freed slot's span is released. The
/// scenario of `sixel_stream.rs`'s
/// `a_resize_inside_a_body_frees_room_the_rest_of_it_can_use` -- an image
/// expired by a resize between two reads of a Sixel body, its span
/// released while the Sixel paints the gap beside it and may then move
/// into it -- is run at every cut with the double lent and without, and
/// every image drawn is the same in both.
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
