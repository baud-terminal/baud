//! The kitty graphics control data, read as the "Control data reference"
//! table gives it (`[[source: kitty-graphics]]`): every key, its default
//! where absent, and what makes a command malformed.

use baud_domain::*;
use proptest::prelude::*;

#[test]
fn every_key_is_read_and_every_absent_key_takes_its_default() {
    let c = KittyCommand::parse(b"");
    assert_eq!(c, KittyCommand::DEFAULT);
    assert_eq!(c.act(), KittyAction::Transmit, "a=t is the default");
    assert_eq!(c.format, KittyFormat::Rgba, "f=32 is the default");
    assert_eq!(c.medium, KittyMedium::Direct, "t=d is the default");
    let c = KittyCommand::parse(
        b"a=T,q=2,f=100,t=d,s=10,v=20,S=30,O=40,i=5,p=6,o=z,m=1,x=1,y=2,w=3,h=4,\
          X=5,Y=6,c=7,r=8,C=1,U=1,z=-1073741825,P=9,Q=10,H=-1,V=2,N=1,d=Z",
    );
    assert_eq!(c.error, None, "{c:?}");
    assert_eq!(c.act(), KittyAction::TransmitPut);
    assert_eq!(c.quiet, KittyQuiet::Silent);
    assert_eq!(c.format, KittyFormat::Png);
    assert_eq!((c.width, c.height, c.size, c.offset), (10, 20, 30, 40));
    assert_eq!((c.id, c.placement), (KittyId::of(5), KittyPlacement::of(6)));
    assert_eq!(
        (c.compression, c.more),
        (KittyCompression::Zlib, KittyMore::More)
    );
    assert_eq!((c.x, c.y, c.w, c.h, c.cell_x, c.cell_y), (1, 2, 3, 4, 5, 6));
    assert_eq!((c.cols, c.rows, c.cursor), (7, 8, KittyCursor::Stay));
    assert_eq!((c.virtual_place, c.z), (1, -1_073_741_825));
    assert_eq!((c.parent, c.parent_placement), (9, 10));
    assert_eq!(c.delete, (KittyDelete::Z, KittyFree::Data));
    let c = KittyCommand::parse(b"I=7,d=r");
    assert_eq!(c.number, KittyNumber::of(7));
    assert_eq!(c.delete, (KittyDelete::Range, KittyFree::Placements));
}

#[test]
fn a_malformed_key_is_an_error_and_an_unknown_one_is_not() {
    for bad in [
        &b"a=x"[..],
        b"f=25",
        b"t=x",
        b"o=y",
        b"d=k",
        b"s=4294967296",
        b"z=2147483648",
        b"s=-1",
        b"s",
        b"i=1,I=2",
    ] {
        let c = KittyCommand::parse(bad);
        assert!(c.error.is_some(), "{}", String::from_utf8_lossy(bad));
    }
    let c = KittyCommand::parse(b",,K=whatever,i=3,");
    assert_eq!(
        c.error, None,
        "empty entries and an unknown key are ignored"
    );
    assert_eq!(c.id, KittyId::of(3), "and the rest is read");
    let c = KittyCommand::parse(b"s=4294967295,z=-2147483648");
    assert_eq!(
        (c.width, c.z, c.error),
        (u32::MAX, i32::MIN, None),
        "32 bits"
    );
    // The ids survive a malformed key, so the error names the image.
    let c = KittyCommand::parse(b"f=7,i=9");
    assert_eq!((c.error, c.id), (Some(KittyError::FORMAT), KittyId::of(9)));
}

/// One `APC G` body through `k`, as the core routes it: `begin`, each
/// byte, then the terminator.
fn command(k: &mut KittyStream, store: &mut ImageStore, body: &[u8]) {
    let at = KittyAt {
        screen: ImageScreen::Primary,
        top: LineIdx::FIRST,
        row: Rows::ZERO,
        col: Col::ZERO,
        span: Cols::of_index(1),
        rows: Rows::of_count(24),
        cols: Cols::of_index(80),
        cell_w: ImagePx::of(10),
        cell_h: ImagePx::of(20),
    };
    let mut scratch = [0u8; 0];
    k.begin(at.screen, store, &mut scratch);
    for &b in body {
        k.put(b, store, &mut scratch);
    }
    k.end(&at, store, &mut scratch);
}

/// **A reply with no room in the outbox is dropped whole and counted**,
/// never cut: an embedder that does not drain the replies between
/// commands. Eight ENOENT replies, each a 51-byte body and its length byte,
/// against 378 bytes of outbox, two of the longest: seven fit, the eighth
/// is counted.
#[test]
fn a_reply_with_no_room_is_dropped_whole_and_counted() {
    let mut k = KittyStream::NEW;
    let (mut px, mut slots, mut places) = (
        [Pixel::CLEAR; 0],
        [ImageSlot::EMPTY; 4],
        [PlacementSlot::EMPTY; 4],
    );
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    let one: &[u8] = b"Gi=1;ENOENT:Put command refers to non-existent image";
    let owed = |k: &KittyStream| k.replies().map(<[u8]>::to_vec).collect::<Vec<_>>();
    for _ in 0..7 {
        command(&mut k, &mut store, b"a=p,i=1");
    }
    assert_eq!(owed(&k), [one; 7], "seven whole replies");
    assert_eq!(k.counts.replies_dropped, 0);
    command(&mut k, &mut store, b"a=p,i=1");
    assert_eq!(owed(&k), [one; 7], "the eighth is not cut in");
    assert_eq!(k.counts.replies_dropped, 1, "and is counted");
    k.clear_replies();
    command(&mut k, &mut store, b"a=p,i=1");
    assert_eq!(owed(&k), [one], "drained, the next fits");
    assert_eq!(k.counts.replies_dropped, 1);
}

/// A reader that has nothing: every read fails.
struct Nothing;

impl KittyFiles for Nothing {
    fn read(
        &mut self,
        _: &KittyFileRequest<'_>,
        _: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), KittyFileFailed> {
        Err(KittyFileFailed)
    }
}

/// **A reader lent serves the file mediums, and `RIS` keeps it**: a read
/// that fails is the one error, where a stream with no reader answers it
/// before reading.
#[test]
fn a_lent_reader_serves_the_file_mediums_and_ris_keeps_it() {
    let mut k = KittyStream::NEW;
    assert_eq!(k.mediums(), KittyMediums::Direct, "Direct by default");
    k.set_files(Box::leak(Box::new(Nothing)));
    assert_eq!(k.mediums(), KittyMediums::Local);
    let (mut px, mut slots, mut places) = (
        [Pixel::CLEAR; 0],
        [ImageSlot::EMPTY; 4],
        [PlacementSlot::EMPTY; 4],
    );
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    command(&mut k, &mut store, b"a=t,i=1,t=f,f=100;L3A=");
    assert_eq!(
        k.replies().collect::<Vec<_>>(),
        [b"Gi=1;EBADF:Failed to read image file".as_slice()]
    );
    k.reset(&mut store, &mut []);
    assert_eq!(k.mediums(), KittyMediums::Local, "RIS keeps the reader");
}

/// **How many bytes a file medium reads**, decided before reading: `S`
/// when given, no more than the object holds, the rest of the object
/// otherwise, and past the limit the limit or a refusal.
#[test]
fn the_read_length_follows_o_s_and_the_limit() {
    let req = |offset, size, limit, past| KittyFileRequest {
        medium: KittyMedium::File,
        path: b"/p",
        offset,
        size,
        limit,
        past,
    };
    let stop = KittyPastLimit::Stop;
    let refuse = KittyPastLimit::Refuse;
    assert_eq!(req(10, 0, 100, refuse).available(50), 40);
    assert_eq!(req(60, 0, 100, refuse).available(50), 0, "O past the end");
    assert_eq!(req(0, 0, 100, refuse).read_len(40), Ok(40), "the rest");
    assert_eq!(req(0, 30, 100, refuse).read_len(40), Ok(30), "S");
    assert_eq!(
        req(0, 41, 100, refuse).read_len(40),
        Ok(40),
        "a claim past the object is a bound, as kitty reads it"
    );
    assert_eq!(req(0, 0, 20, stop).read_len(40), Ok(20), "to the limit");
    assert_eq!(req(0, 0, 20, refuse).read_len(40), Err(KittyFileFailed));
    assert_eq!(req(0, 0, 40, refuse).read_len(40), Ok(40), "at the limit");
}

/// kitty's read of a file medium (`[[source: kitty-graphics-c]]`
/// `read_img_file`): `S`, or the bytes after `O` when it is zero, no more
/// than `max_to_read`, then a read that stops at the end of the object.
fn kittys_read(offset: u64, size: u64, len: u64, max_to_read: u64) -> u64 {
    let available = len.saturating_sub(offset);
    let sz = match size {
        0 => available,
        s => s,
    };
    sz.min(max_to_read).min(available)
}

proptest! {
    /// **The read length is kitty's**, for `S` below, at and above the
    /// bytes after `O`, and `O` inside and past the end: where the limit
    /// stops a read, exactly kitty's count, and where it refuses one,
    /// kitty's count wherever the limit is not passed.
    #[test]
    fn the_read_length_is_kittys_clamp(
        len in 0u64..64,
        o_past in 0u64..80,
        s_past in 0u64..80,
        limit in 0usize..80,
    ) {
        let offset = o_past.min(len + 16);
        let available = len.saturating_sub(offset);
        // `S` from none through the bytes there to past them.
        let size = s_past.min(available + 16);
        let req = |past| KittyFileRequest {
            medium: KittyMedium::TempFile,
            path: b"/tmp/tty-graphics-protocol-x",
            offset: u32::try_from(offset).unwrap(),
            size: u32::try_from(size).unwrap(),
            limit,
            past,
        };
        let r = req(KittyPastLimit::Stop);
        prop_assert_eq!(r.available(len), available);
        let kitty = usize::try_from(kittys_read(offset, size, len, u64::try_from(limit).unwrap())).unwrap();
        prop_assert_eq!(r.read_len(available), Ok(kitty));
        let unbounded = usize::try_from(kittys_read(offset, size, len, u64::MAX)).unwrap();
        let refused = req(KittyPastLimit::Refuse).read_len(available);
        match unbounded > limit {
            true => prop_assert_eq!(refused, Err(KittyFileFailed)),
            false => prop_assert_eq!(refused, Ok(kitty)),
        }
    }
}

/// Where a command's image is placed in every transmission below.
fn place() -> KittyAt {
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

/// A reader that serves each path as the file's bytes, read as the request
/// bounds them.
struct Echo;

impl KittyFiles for Echo {
    fn read(
        &mut self,
        req: &KittyFileRequest<'_>,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), KittyFileFailed> {
        let n = req.read_len(u64::try_from(req.path.len()).unwrap())?;
        sink(&req.path[..n]);
        Ok(())
    }
}

/// What a stream made of a transmission: its replies, its counts and the
/// pixels of every placed image.
#[derive(Debug, PartialEq)]
struct Made {
    replies: Vec<Vec<u8>>,
    counts: KittyCounts,
    drawn: Vec<Vec<u32>>,
}

/// How the body bytes reach the stream.
#[derive(Clone, Copy)]
enum Feed<'c> {
    /// [`KittyStream::put`], a byte at a time.
    Bytes,
    /// [`KittyStream::put_run`], each body cut at these offsets.
    Runs(&'c [usize]),
}

/// **`chunks` (each an `APC G` body after its `G`) through a fresh stream**,
/// with `scratch_len` bytes of scratch, each terminated, then every decode
/// stepped to its end in small steps.
fn made(chunks: &[Vec<u8>], scratch_len: usize, feed: Feed<'_>, files: bool) -> Made {
    let mut px = vec![Pixel::CLEAR; 256];
    let (mut slots, mut places) = ([ImageSlot::EMPTY; 4], [PlacementSlot::EMPTY; 8]);
    let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
    let mut k = KittyStream::NEW;
    if files {
        k.set_files(Box::leak(Box::new(Echo)));
    }
    let mut scratch = vec![0u8; scratch_len];
    for body in chunks {
        k.begin(ImageScreen::Primary, &mut store, &mut scratch);
        match feed {
            Feed::Bytes => {
                for &b in body {
                    k.put(b, &mut store, &mut scratch);
                }
            }
            Feed::Runs(cuts) => {
                let mut ends: Vec<usize> = cuts.iter().map(|c| c % (body.len() + 1)).collect();
                ends.push(body.len());
                ends.sort_unstable();
                let mut from = 0;
                for to in ends {
                    if to > from {
                        k.put_run(&body[from..to], &mut store, &mut scratch);
                        from = to;
                    }
                }
            }
        }
        k.end(&place(), &mut store, &mut scratch);
    }
    for _ in 0..100_000 {
        let left = k.step(DecodeBudget::of(97), &mut store, &mut scratch);
        if left == DecodeProgress::Idle {
            break;
        }
    }
    let drawn = store
        .placements()
        .filter_map(|(_, p)| store.image(p.image))
        .map(|v| v.pixels.iter().map(|p| p.bits()).collect())
        .collect();
    Made {
        replies: k.replies().map(<[u8]>::to_vec).collect(),
        counts: k.counts,
        drawn,
    }
}

/// `bytes` in `[[source: rfc4648-base64]]`'s alphabet, padded.
fn base64(bytes: &[u8]) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    for q in bytes.chunks(3) {
        let w = q
            .iter()
            .enumerate()
            .fold(0u32, |w, (i, &b)| w | u32::from(b) << (16 - 8 * i));
        let sextet = |i: u32| A[usize::try_from(w >> (18 - 6 * i) & 63).unwrap()];
        out.extend((0..=u32::try_from(q.len()).unwrap()).map(sextet));
        out.extend(std::iter::repeat_n(b'=', 3 - q.len()));
    }
    out
}

/// The PNG CRC (ISO 3309), a bit at a time.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = match c & 1 {
                1 => 0xEDB8_8320 ^ (c >> 1),
                _ => c >> 1,
            };
        }
    }
    !c
}

/// A PNG chunk of `kind` holding `data`.
fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut c = u32::try_from(data.len()).unwrap().to_be_bytes().to_vec();
    c.extend_from_slice(kind);
    c.extend_from_slice(data);
    let crc = crc32(&c[4..]);
    c.extend_from_slice(&crc.to_be_bytes());
    c
}

/// `n` bytes from `seed`.
fn bytes_from(seed: u64, n: usize) -> Vec<u8> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            s.to_le_bytes()[5]
        })
        .collect()
}

/// A `w`x`h` RGBA8 PNG of `seed`'s pixels, behind a `tEXt` of `text`
/// bytes that sets its length.
fn png(w: u32, h: u32, text: usize, seed: u64) -> Vec<u8> {
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let row = usize::try_from(w).unwrap() * 4;
    let mut raw = Vec::new();
    for r in 0..usize::try_from(h).unwrap() {
        raw.push(0);
        raw.extend(bytes_from(seed ^ u64::try_from(r).unwrap(), row));
    }
    let mut out = vec![0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];
    out.extend(png_chunk(b"IHDR", &ihdr));
    let mut t = b"k\0".to_vec();
    t.extend(std::iter::repeat_n(b'v', text));
    out.extend(png_chunk(b"tEXt", &t));
    out.extend(png_chunk(
        b"IDAT",
        &miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6),
    ));
    out.extend(png_chunk(b"IEND", &[]));
    out
}

/// The transmissions the property runs: the control data of the first
/// chunk, the data, and whether it is a file medium's.
fn transmission(kind: usize, w: u32, h: u32, seed: u64, text: usize) -> (String, Vec<u8>, bool) {
    let n = usize::try_from(w * h).unwrap();
    let image = png(w, h, text, seed);
    let z = |b: &[u8]| miniz_oxide::deflate::compress_to_vec_zlib(b, 6);
    let rgb = bytes_from(seed, n * 3);
    let rgba = bytes_from(seed, n * 4);
    let raw = |f: u32| format!("a=T,i=1,f={f},s={w},v={h}");
    match kind {
        0 => ("a=T,i=1,f=100".into(), image, false),
        1 => (
            format!("a=T,i=1,f=100,o=z,S={}", image.len()),
            z(&image),
            false,
        ),
        2 => (raw(24) + ",o=z", z(&rgb), false),
        3 => (raw(32) + ",o=z", z(&rgba), false),
        4 => (raw(24), rgb, false),
        5 => (raw(32), rgba, false),
        6 => (raw(24) + ",t=f", rgb, true),
        _ => {
            // A path past the path buffer.
            let mut long = rgb;
            long.extend(bytes_from(seed, KITTY_PATH_MAX));
            (raw(24) + ",t=f", long, true)
        }
    }
}

/// The bytes spliced into a payload: `=`, and bytes that are no base64.
const SPLICED: [u8; 6] = [b'=', b' ', b'!', b'-', b'.', 0xC3];

/// **The chunks of `payload`**, cut at `cuts`, the first carrying
/// `control`, each but the last `m=1`.
fn chunked(control: &str, payload: &[u8], cuts: &[usize]) -> Vec<Vec<u8>> {
    let mut ends: Vec<usize> = cuts.iter().map(|c| c % (payload.len() + 1)).collect();
    ends.push(payload.len());
    ends.sort_unstable();
    ends.dedup();
    let last = ends.len() - 1;
    let mut from = 0;
    let mut out = Vec::new();
    for (i, to) in ends.into_iter().enumerate() {
        let m = match i == last {
            true => "m=0",
            false => "m=1",
        };
        let head = match i {
            0 => format!("{control},{m};"),
            _ => format!("{m};"),
        };
        let mut body = head.into_bytes();
        body.extend_from_slice(&payload[from..to]);
        out.push(body);
        from = to;
    }
    out
}

proptest! {
    /// **A payload's octets are the same wherever the reads fell**:
    /// [`KittyStream::put_run`] over any cuts makes what `put` makes a byte
    /// at a time -- the replies, the counts, the pixels -- for a PNG direct
    /// and through zlib, RGB and RGBA through zlib and direct, and a file
    /// medium's path, short and past the path buffer; mostly the alphabet,
    /// with `=` and bytes that are no base64 spliced in and the chunks cut
    /// anywhere, so their tails are short; with the scratch large, near the
    /// PNG's length and small.
    #[test]
    fn kitty_octets_are_the_same_wherever_the_reads_fell(
        kind in 0usize..8,
        (w, h) in (1u32..6, 1u32..6),
        seed in any::<u64>(),
        text in 0usize..3,
        splices in prop::collection::vec((any::<usize>(), 0usize..SPLICED.len()), 0..3),
        chunk_cuts in prop::collection::vec(any::<usize>(), 0..3),
        run_cuts in prop::collection::vec(any::<usize>(), 0..12),
        (pick, d) in (0u8..3, 0usize..48),
    ) {
        let (control, data, files) = transmission(kind, w, h, seed, text);
        let mut payload = base64(&data);
        for (at, b) in splices {
            payload.insert(at % (payload.len() + 1), SPLICED[b]);
        }
        let chunks = chunked(&control, &payload, &chunk_cuts);
        let scratch = match pick {
            0 => 4096,
            1 => (data.len() + d % 7).saturating_sub(3),
            _ => d,
        };
        let bytes = made(&chunks, scratch, Feed::Bytes, files);
        prop_assert_eq!(made(&chunks, scratch, Feed::Runs(&run_cuts), files), bytes);
    }
}

/// **A run cut inside a quantum, 1+3, 2+2 and 3+1**, before and after a
/// PNG's 24th byte and in an inflate's stage, makes what `put` makes; and
/// the image is drawn, the control that the transmission decodes at all.
#[test]
fn a_quantum_split_across_runs_is_the_same_as_bytes() {
    for kind in [0, 2] {
        let (control, data, files) = transmission(kind, 4, 3, 7, 0);
        let chunks = chunked(&control, &base64(&data), &[]);
        let start = control.len() + ",m=0;".len();
        let bytes = made(&chunks, 4096, Feed::Bytes, files);
        assert_eq!(bytes.drawn.len(), 1, "the control: kind {kind} draws");
        for quantum in [5, 40] {
            for split in 1..4 {
                let cut = [start + 4 * quantum + split];
                let runs = made(&chunks, 4096, Feed::Runs(&cut), files);
                assert_eq!(
                    runs,
                    bytes,
                    "kind {kind}, quantum {quantum}, {split}+{}",
                    4 - split
                );
            }
        }
    }
}

/// **The scratch's end, two bytes either side of a PNG's length**, with
/// its data ending on a whole quantum, on `xxx=`, on `xx==` and on a
/// lone character and a pad (`x=`), in one run: the same as `put` a byte
/// at a time; and past it, where the PNG is drawn exactly when the scratch
/// holds it and its scanline.
#[test]
fn a_png_at_the_scratch_end_is_the_same_as_bytes() {
    let mut tails = 0;
    for text in 0..3 {
        let data = png(1, 1, text, 3);
        let mut encodings = vec![base64(&data)];
        if data.len() % 3 == 0 {
            let mut lone = base64(&data);
            lone.extend_from_slice(b"Q=");
            encodings.push(lone);
        }
        for payload in encodings {
            tails += 1;
            let chunks = chunked("a=T,i=1,f=100", &payload, &[]);
            let edge = data.len() - 2..=data.len() + 2;
            for cap in edge.chain(data.len() + 4..=data.len() + 6) {
                let bytes = made(&chunks, cap, Feed::Bytes, false);
                let runs = make_one_run(&chunks, cap);
                assert_eq!(runs, bytes, "{} bytes, scratch {cap}", data.len());
                // A 1x1 RGBA PNG's one scanline is a filter byte and four.
                assert_eq!(bytes.drawn.len(), usize::from(cap >= data.len() + 5));
            }
        }
    }
    assert_eq!(tails, 4, "whole, xxx=, xx== and x=");
}

/// `chunks` through `put_run`, each body in one run.
fn make_one_run(chunks: &[Vec<u8>], cap: usize) -> Made {
    made(chunks, cap, Feed::Runs(&[]), false)
}

thread_local! {
    /// The longest span this thread's releases were handed.
    static RELEASED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A release of one-byte pages that records the span it was handed.
fn record(span: &mut [u8]) -> PagesBack {
    RELEASED.with(|r| r.set(r.get().max(span.len())));
    PagesBack::Kept
}

/// What the scratch holds where nothing has written.
const UNWRITTEN: u8 = 0xA5;

/// What a refusal must leave as it was: the images the two ids name, the
/// pixels of every placement, the placements and the store's epoch.
type StoreState = (
    Option<ImageId>,
    Option<ImageId>,
    Vec<Vec<u32>>,
    usize,
    ImageGen,
);

fn store_state(store: &ImageStore) -> StoreState {
    let drawn = store
        .placements()
        .filter_map(|(_, p)| store.image(p.image))
        .map(|v| v.pixels.iter().map(|p| p.bits()).collect())
        .collect();
    (
        store.kitty_image(KittyId::of(1)),
        store.kitty_image(KittyId::of(2)),
        drawn,
        store.placements().count(),
        store.generation(),
    )
}

/// **A refused PNG changes nothing and gives the scratch back up to the
/// highest byte it wrote**, a byte at a time and in one run: refused too
/// large past a 40-byte scratch, the last byte written its 40th; refused at
/// its header's width, past 65535, the last written its 24th; and cut short
/// by a byte that is no base64 nine octets in, refused at its end, the last
/// written its 9th. Image 1, placed before, is still the one image, drawn
/// as it was.
#[test]
fn a_refused_png_releases_the_scratch_it_wrote() {
    let image = png(2, 2, 0, 5);
    // 65536 pixels wide: past what a side may be, refused at its 24th byte.
    let mut too_wide = image.clone();
    too_wide[16..20].copy_from_slice(&65_536u32.to_be_bytes());
    let mut cut = base64(&image);
    cut.insert(12, b'!');
    let cases = [
        (base64(&image), 40, 39),
        (base64(&too_wide), 4096, 23),
        (cut, 4096, 8),
    ];
    for (payload, cap, want) in cases {
        let body = chunked("a=T,i=2,f=100", &payload, &[]).remove(0);
        for feed in [Feed::Bytes, Feed::Runs(&[])] {
            let mut px = vec![Pixel::CLEAR; 256];
            let (mut slots, mut places) = ([ImageSlot::EMPTY; 4], [PlacementSlot::EMPTY; 8]);
            let mut places_index = vec![PlacementId::NONE; PlacementId::index_len(places.len())];
            let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut places_index);
            store.set_release(PageRelease::new(1, record).unwrap());
            let mut k = KittyStream::NEW;
            let mut scratch = vec![UNWRITTEN; cap];
            let before_cmd = b"a=T,i=1,f=24,s=1,v=1,q=2;/wAA";
            k.begin(ImageScreen::Primary, &mut store, &mut scratch);
            k.put_run(before_cmd, &mut store, &mut scratch);
            k.end(&place(), &mut store, &mut scratch);
            let before = store_state(&store);
            assert_eq!(
                before.2,
                [vec![0xffff_0000]],
                "the control: image 1 is drawn"
            );
            RELEASED.with(|r| r.set(0));
            k.begin(ImageScreen::Primary, &mut store, &mut scratch);
            match feed {
                Feed::Bytes => {
                    for &b in &body {
                        k.put(b, &mut store, &mut scratch);
                    }
                }
                Feed::Runs(_) => {
                    k.put_run(&body, &mut store, &mut scratch);
                }
            }
            k.end(&place(), &mut store, &mut scratch);
            while k.step(DecodeBudget::ALL, &mut store, &mut scratch) == DecodeProgress::More {}
            let replies: Vec<Vec<u8>> = k.replies().map(<[u8]>::to_vec).collect();
            assert!(
                replies.len() == 1 && replies[0].starts_with(b"Gi=2;E"),
                "refused: {replies:?}"
            );
            assert_eq!(store_state(&store), before, "the refusal changed the store");
            let highest = scratch.iter().rposition(|&b| b != UNWRITTEN);
            assert_eq!(highest, Some(want), "the control: the bytes written");
            let released = RELEASED.with(std::cell::Cell::get);
            assert!(released > want, "released {released}, wrote to {want}");
        }
    }
}
