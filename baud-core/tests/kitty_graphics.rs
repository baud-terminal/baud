//! The kitty graphics protocol through the byte stream
//! (`[[source: kitty-graphics]]`; note V, V19 steps 5-6): queries answered,
//! images stored, placed and deleted, the deferred decode, and every
//! transmission the same however its bytes were cut -- across the `APC`
//! body, across base64 quanta, across zlib blocks and across `m=1` chunks.
//!
//! The pictures are written by oracles, never by the decoder under test: the
//! `png` crate encodes the PNGs, `miniz_oxide` the zlib streams, and the
//! expected pixels are the source bytes themselves.

use baud_core::*;
use baud_domain::{
    CacheGeometry, DecodeBudget, DecodeProgress, Depth, ImageAlpha, ImageBudget, ImagePx,
    KittyPlacement, LineIdx, Pixel,
};
use std::fmt::Write as _;
use std::io::Write as _;

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// A terminal with `pixels` of arena and a 10x20 cell.
fn with<R>(pixels: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(pixels));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    f(&mut t)
}

/// Feed `bytes` as one read, keeping the replies.
fn feed(t: &mut Term<'_>, bytes: &[u8], replies: &mut Vec<u8>) {
    t.write_at(BytePos::ZERO, bytes);
    t.feed(BytePos::ZERO, ReadLen::from_index(bytes.len()));
    replies.extend_from_slice(t.reply());
    t.clear_reply();
}

/// Finish any deferred decode.
fn settle(t: &mut Term<'_>, replies: &mut Vec<u8>) {
    while t.decode_step(DecodeBudget::ALL) == DecodeProgress::More {}
    replies.extend_from_slice(t.reply());
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
            if k <= q.len() {
                let six = usize::from(u8::try_from((n >> (18 - 6 * k)) & 63).unwrap());
                out.push(B64[six]);
            } else {
                out.push(b'=');
            }
        }
    }
    out
}

/// Base64 with the pads dropped, as kitten writes it.
fn b64_unpadded(data: &[u8]) -> Vec<u8> {
    let mut v = b64(data);
    while v.last() == Some(&b'=') {
        v.pop();
    }
    v
}

/// One graphics command.
fn apc(ctrl: &str, payload: &[u8]) -> Vec<u8> {
    let mut v = b"\x1b_G".to_vec();
    v.extend_from_slice(ctrl.as_bytes());
    if !payload.is_empty() {
        v.push(b';');
        v.extend_from_slice(payload);
    }
    v.extend_from_slice(b"\x1b\\");
    v
}

/// A payload cut into chunks of `n` encoded bytes: the first carries
/// `first`, the rest `rest`, each `m=1` but the last.
///
/// **`n` is rounded up to a multiple of 4**: "chunks, except the last, must
/// have a size that is a multiple of 4" (`[[source: kitty-graphics-rst]]`),
/// and kitty decodes each chunk on its own (`KittyBase64`), so a chunk cut
/// inside a quantum loses that quantum's bits, as kitty loses them.
fn chunked(first: &str, rest: &str, encoded: &[u8], n: usize) -> Vec<u8> {
    let pieces: Vec<&[u8]> = encoded.chunks(n.div_ceil(4) * 4).collect();
    let mut v = Vec::new();
    for (k, piece) in pieces.iter().enumerate() {
        let more = if k + 1 == pieces.len() { "m=0" } else { "m=1" };
        let keys = if k == 0 { first } else { rest };
        let sep = if keys.is_empty() { "" } else { "," };
        v.extend(apc(&format!("{keys}{sep}{more}"), piece));
    }
    v
}

/// A zlib stream of `data` in three parts: stored, then the strongest
/// level, then the fastest, each part ended by a sync flush -- so stored,
/// fixed or dynamic blocks and the empty stored block a sync flush writes
/// all occur in one stream.
fn zlib_mixed(data: &[u8]) -> Vec<u8> {
    use miniz_oxide::deflate::core::{CompressorOxide, TDEFLFlush, TDEFLStatus, compress};
    let mut c = CompressorOxide::default();
    c.set_format_and_level(miniz_oxide::DataFormat::Zlib, 0);
    let third = data.len() / 3;
    let parts = [
        (&data[..third], 0u8, TDEFLFlush::Sync),
        (&data[third..2 * third], 9, TDEFLFlush::Sync),
        (&data[2 * third..], 1, TDEFLFlush::Finish),
    ];
    let mut out = Vec::new();
    for (part, level, flush) in parts {
        c.set_compression_level_raw(level);
        let mut input = part;
        loop {
            let mut buf = vec![0u8; 1 << 16];
            let (status, read, wrote) = compress(&mut c, input, &mut buf, flush);
            out.extend_from_slice(&buf[..wrote]);
            input = &input[read..];
            if status == TDEFLStatus::Done || (input.is_empty() && status == TDEFLStatus::Okay) {
                break;
            }
            assert!(status != TDEFLStatus::BadParam, "compress");
        }
    }
    out
}

/// A PNG of 8-bit `data` in `color`, its stream cut into `IDAT` chunks of
/// `idat` bytes.
fn png(w: u32, h: u32, color: png::ColorType, data: &[u8], idat: usize) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(color);
        e.set_depth(png::BitDepth::Eight);
        // Every filter a row can carry, so the unfiltering is exercised
        // and not only `Sub`, the crate's default.
        e.set_filter(png::Filter::Adaptive);
        let mut wr = e.write_header().unwrap();
        let mut sw = wr.stream_writer_with_size(idat).unwrap();
        sw.write_all(data).unwrap();
        sw.finish().unwrap();
    }
    out
}

/// Deterministic bytes, a mix of runs and noise so zlib makes every block
/// kind.
fn bytes(n: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).max(1);
    (0..n)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            match (i / 37) % 3 {
                0 => u8::try_from(i % 7).unwrap(),
                _ => x.to_le_bytes()[0],
            }
        })
        .collect()
}

/// RGBA bytes as the straight ARGB pixels the store keeps.
fn rgba_pixels(data: &[u8]) -> Vec<u32> {
    let c = Chan::from_u8;
    data.chunks(4)
        .map(|p| Pixel::straight(c(p[3]), c(p[0]), c(p[1]), c(p[2])).bits())
        .collect()
}

/// RGB bytes as opaque pixels.
fn rgb_pixels(data: &[u8]) -> Vec<u32> {
    let c = Chan::from_u8;
    data.chunks(3)
        .map(|p| Pixel::opaque(c(p[0]), c(p[1]), c(p[2])).bits())
        .collect()
}

/// Everything the protocol leaves behind, as text to compare: each
/// placement with its geometry and its image's pixels, the cursor, the
/// replies and the counts.
fn state(t: &Term<'_>, replies: &[u8]) -> String {
    let mut s = String::new();
    let imgs = t.images();
    for (_, p) in imgs.placements() {
        let _ = write!(
            s,
            "place {:?} {:?} {} {:?} {:?} ",
            p.screen,
            p.line,
            p.col.index(),
            p.z,
            p.kitty
        );
        match imgs.image(p.image) {
            Some(v) => {
                let px: Vec<u32> = v.pixels.iter().map(|p| p.bits()).collect();
                let _ = writeln!(
                    s,
                    "{}x{} {:?} {:?}",
                    v.width.raw(),
                    v.height.raw(),
                    v.alpha,
                    px
                );
            }
            None => s.push_str("decoding\n"),
        }
    }
    let _ = writeln!(
        s,
        "cursor {:?} replies {:?} counts {:?} refused {} evicted {}",
        t.cursor(),
        String::from_utf8_lossy(replies),
        t.kitty(),
        imgs.images_refused,
        imgs.images_evicted
    );
    s
}

/// The pixels of the one image placed.
fn placed_pixels(t: &Term<'_>) -> Vec<u32> {
    let imgs = t.images();
    let (_, p) = imgs.placements().next().expect("a placement");
    let v = imgs.image(p.image).expect("decoded");
    v.pixels.iter().map(|p| p.bits()).collect()
}

/// **Every read size and every two-read split**: the state after `stream`
/// fed whole is the state after it fed `n` bytes at a time for every `n`,
/// and in two reads cut at every byte.
fn invariant(pixels: usize, stream: &[u8], every_split: Splits) {
    let whole = with(pixels, |t| {
        let mut r = Vec::new();
        feed(t, stream, &mut r);
        settle(t, &mut r);
        state(t, &r)
    });
    let sizes: Vec<usize> = match every_split {
        Splits::Every => (1..=stream.len()).collect(),
        Splits::Some => vec![1, 2, 3, 5, 7, 64, 1023, 1024, 4095, 4096, 65536],
    };
    for n in sizes {
        let got = with(pixels, |t| {
            let mut r = Vec::new();
            for piece in stream.chunks(n) {
                feed(t, piece, &mut r);
            }
            settle(t, &mut r);
            state(t, &r)
        });
        assert_eq!(got, whole, "read size {n}");
    }
    let cuts: Vec<usize> = match every_split {
        Splits::Every => (1..stream.len()).collect(),
        Splits::Some => (1..stream.len()).step_by(stream.len() / 97 + 1).collect(),
    };
    for cut in cuts {
        let got = with(pixels, |t| {
            let mut r = Vec::new();
            feed(t, &stream[..cut], &mut r);
            feed(t, &stream[cut..], &mut r);
            settle(t, &mut r);
            state(t, &r)
        });
        assert_eq!(got, whole, "two reads cut at {cut}");
    }
}

#[derive(Clone, Copy)]
enum Splits {
    Every,
    Some,
}

#[test]
fn kittens_detection_is_answered_before_the_primary_da() {
    // `[[source: kitty-icat-detect]]`'s burst: one query a medium, then DA.
    let mut s = apc("i=31,s=1,v=1,a=q,t=d,f=24", b"AAAA");
    s.extend(apc(
        "i=32,s=1,v=1,a=q,t=t,f=24",
        &b64(b"/tmp/tty-graphics-protocol-x"),
    ));
    s.extend(apc("i=33,s=1,v=1,a=q,t=s,f=24", &b64(b"/shm")));
    s.extend_from_slice(b"\x1b[c");
    let r = with(64, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        r
    });
    let r = String::from_utf8_lossy(&r).into_owned();
    let expect = "\x1b_Gi=31;OK\x1b\\\
        \x1b_Gi=32;EBADF:Failed to read image file\x1b\\\
        \x1b_Gi=33;EBADF:Failed to read image file\x1b\\";
    assert!(
        r.starts_with(expect),
        "the three answers first, in order: {r:?}"
    );
    assert!(
        r[expect.len()..].starts_with("\x1b[?"),
        "then the DA: {r:?}"
    );
}

#[test]
fn a_query_stores_nothing_and_a_png_query_is_answered_at_its_terminator() {
    let p = png(2, 2, png::ColorType::Rgba, &bytes(16, 3), 8);
    let s = apc("a=q,i=7,f=100", &b64(&p));
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        assert_eq!(r, b"\x1b_Gi=7;OK\x1b\\", "answered before any step");
        assert_eq!(t.images().placements().count(), 0, "nothing placed");
        // Nothing stored: the whole arena is free for a 64-pixel image.
        feed(t, &apc("a=T,f=24,s=8,v=8", &b64(&[9u8; 192])), &mut r);
        assert_eq!(t.images().images_refused, 0, "the query left no image");
    });
    // A broken PNG is an error, not silence.
    let mut bad = p.clone();
    bad[40] ^= 0xFF;
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=q,i=8,f=100", &b64(&bad)), &mut r);
        assert!(
            r.starts_with(b"\x1b_Gi=8;EBADPNG:"),
            "{:?}",
            String::from_utf8_lossy(&r)
        );
    });
}

#[test]
fn raw_rgba_and_rgb_are_stored_placed_and_move_the_cursor() {
    let rgba = bytes(3 * 2 * 4, 11);
    let rgb = bytes(3 * 2 * 3, 12);
    with(256, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[5;7H", &mut r);
        feed(t, &apc("a=T,f=32,s=3,v=2,i=1", &b64(&rgba)), &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
        assert_eq!(placed_pixels(t), rgba_pixels(&rgba));
        let imgs = t.images();
        let (_, p) = imgs.placements().next().unwrap();
        assert_eq!(
            p.line,
            LineIdx::FIRST.plus(Rows::of_count(4)),
            "the cursor's line"
        );
        assert_eq!(p.col.index(), 6, "the cursor's column");
        // 3x2 pixels in 10x20 cells: one column and one row, so the cursor
        // goes right one and down none, the rows less one, as kitty moves it
        // (`[[source: kitty-graphics-c]]`, `handle_put_command`); it went down
        // one, to (5, 7), by the text's "down by the number of rows".
        assert_eq!(t.cursor(), (Row::from_index(4), Col::at_index(7)));
    });
    with(256, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=T,f=24,s=3,v=2,C=1", &b64(&rgb)), &mut r);
        assert!(r.is_empty(), "no id, no reply");
        assert_eq!(placed_pixels(t), rgb_pixels(&rgb));
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO), "C=1 leaves the cursor");
    });
}

#[test]
fn each_image_is_marked_by_its_own_pixels() {
    let opaque = [1u8, 2, 3, 255, 4, 5, 6, 0];
    let partial = [1u8, 2, 3, 255, 4, 5, 6, 128];
    for (data, mark) in [
        (&opaque, ImageAlpha::OpaqueOrClear),
        (&partial, ImageAlpha::Partial),
    ] {
        with(64, |t| {
            let mut r = Vec::new();
            feed(t, &apc("a=T,f=32,s=2,v=1", &b64(data)), &mut r);
            let imgs = t.images();
            let (_, p) = imgs.placements().next().unwrap();
            assert_eq!(imgs.image(p.image).unwrap().alpha, mark);
        });
    }
}

/// A raw RGBA image in chunks of 5 encoded bytes, so base64 quanta and
/// pixels both straddle chunks, with kitten's repeated keys on each.
#[test]
fn raw_chunks_are_the_same_however_the_reads_fall() {
    let data = bytes(4 * 3 * 4, 21);
    let s = chunked("a=T,f=32,s=4,v=3,i=5", "a=T,f=32", &b64(&data), 5);
    invariant(64, &s, Splits::Every);
    let got = with(64, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        (placed_pixels(t), r)
    });
    assert_eq!(got.0, rgba_pixels(&data), "the pixels sent");
    assert_eq!(got.1, b"\x1b_Gi=5;OK\x1b\\", "one reply, at the last chunk");
}

#[test]
fn zlib_rgb_crosses_every_block_kind_and_every_cut() {
    let data = bytes(6 * 5 * 3, 31);
    let z = zlib_mixed(&data);
    let s = chunked("a=T,f=24,s=6,v=5,o=z,i=6", "", &b64_unpadded(&z), 7);
    invariant(64, &s, Splits::Every);
    let got = with(64, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        settle(t, &mut r);
        (placed_pixels(t), r)
    });
    assert_eq!(got.0, rgb_pixels(&data));
    assert_eq!(got.1, b"\x1b_Gi=6;OK\x1b\\");
}

#[test]
fn zlib_rgba_and_png_through_zlib_are_chunk_invariant() {
    let data = bytes(5 * 4 * 4, 41);
    let s = chunked("a=T,f=32,s=5,v=4,o=z", "", &b64(&zlib_mixed(&data)), 9);
    invariant(64, &s, Splits::Every);
    let p = png(5, 4, png::ColorType::Rgba, &data, 13);
    let z = zlib_mixed(&p);
    let ctrl = format!("a=T,f=100,o=z,S={},i=9", p.len());
    let s = chunked(&ctrl, "", &b64(&z), 11);
    invariant(256, &s, Splits::Every);
    let got = with(256, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        settle(t, &mut r);
        placed_pixels(t)
    });
    assert_eq!(got, rgba_pixels(&data));
}

#[test]
fn a_png_across_idat_chunks_is_chunk_invariant() {
    let data = bytes(7 * 5 * 3, 51);
    let p = png(7, 5, png::ColorType::Rgb, &data, 10);
    let s = chunked("a=T,f=100,i=10", "", &b64(&p), 6);
    invariant(256, &s, Splits::Every);
    let got = with(256, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        settle(t, &mut r);
        (placed_pixels(t), r)
    });
    assert_eq!(got.0, rgb_pixels(&data));
    assert_eq!(got.1, b"\x1b_Gi=10;OK\x1b\\");
}

/// **What `kitten icat` sends** (`[[source: kitty-graphics-command]]`):
/// unpadded base64 in chunks of 131,072 bytes, `a=T` on every one.
#[test]
fn a_kitten_shaped_stream_is_chunk_invariant() {
    let data = bytes(256 * 192 * 4, 61);
    let p = png(256, 192, png::ColorType::Rgba, &data, 8192);
    let encoded = b64_unpadded(&p);
    let s = chunked("a=T,f=100,q=2", "a=T,f=100,q=2", &encoded, 131_072);
    assert!(encoded.len() > 131_072, "more than one chunk");
    invariant(256 * 192, &s, Splits::Some);
    let got = with(256 * 192, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        settle(t, &mut r);
        placed_pixels(t)
    });
    assert_eq!(got, rgba_pixels(&data));
}

/// **The decode is deferred, and stepped**: the image is placed and the
/// cursor moved at the last chunk, shown only when every pixel is made, and
/// answered then; a step of one byte makes the same picture as one of all.
#[test]
fn a_png_is_placed_at_once_and_shown_when_its_steps_complete() {
    let data = bytes(21 * 23 * 4, 71);
    let p = png(21, 23, png::ColorType::Rgba, &data, 50);
    let s = apc("a=T,f=100,i=12", &b64(&p));
    let (stepped, steps) = with(1024, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        assert!(r.is_empty(), "no reply before the decode completes");
        let imgs = t.images();
        let (_, pl) = imgs.placements().next().expect("placed at once");
        assert!(imgs.image(pl.image).is_none(), "not shown while decoding");
        // Two rows, so down one, as kitty moves it; down two, to (2, 3),
        // by the text's "down by the number of rows".
        assert_eq!(
            t.cursor(),
            (Row::from_index(1), Col::at_index(3)),
            "moved at once: three columns, two rows"
        );
        let mut steps = 0;
        while t.decode_step(DecodeBudget::of(1)) == DecodeProgress::More {
            steps += 1;
        }
        r.extend_from_slice(t.reply());
        assert_eq!(r, b"\x1b_Gi=12;OK\x1b\\", "answered at completion");
        (placed_pixels(t), steps)
    });
    assert!(steps > 100, "a byte at a time takes many steps: {steps}");
    let whole = with(1024, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        settle(t, &mut r);
        placed_pixels(t)
    });
    assert_eq!(stepped, whole, "the same picture");
    assert_eq!(whole, rgba_pixels(&data), "the picture sent");
}

#[test]
fn a_command_finishes_the_pending_decode_first_so_replies_keep_order() {
    let p = png(4, 4, png::ColorType::Rgb, &bytes(48, 81), 20);
    let mut s = apc("a=T,f=100,i=1", &b64(&p));
    s.extend(apc("a=p,i=1,p=3", b""));
    with(256, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\\x1b_Gi=1,p=3;OK\x1b\\");
        assert_eq!(t.images().placements().count(), 2);
    });
}

#[test]
fn every_declared_size_is_checked_before_anything_is_reserved() {
    // The arena holds 64 pixels: 9x8 is refused by `s`/`v`, by a PNG's
    // `IHDR`, and a `S` past the scratch is refused too, each answered. The
    // PNG carries a text chunk longer than the 512-byte scratch after its
    // header, so a check that waited for the last chunk would answer EFBIG
    // for the scratch rather than EINVAL for the size.
    let big = {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 9, 8);
        e.set_color(png::ColorType::Rgb);
        e.set_depth(png::BitDepth::Eight);
        e.add_text_chunk("Comment".into(), "x".repeat(2000))
            .unwrap();
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&bytes(216, 1)).unwrap();
        drop(wr);
        out
    };
    for (s, code) in [
        (apc("a=T,f=24,s=9,v=8,i=1", &b64(&bytes(216, 1))), "EINVAL"),
        (apc("a=T,f=100,i=1", &b64(&big)), "EINVAL"),
        (
            apc("a=T,f=100,o=z,S=99999,i=1", &b64(&zlib_mixed(&big))),
            "EINVAL",
        ),
        (apc("a=T,f=24,s=0,v=8,i=1", b""), "EINVAL"),
    ] {
        with(64, |t| {
            let mut r = Vec::new();
            feed(t, &s, &mut r);
            settle(t, &mut r);
            let reply = String::from_utf8_lossy(&r).into_owned();
            assert!(
                reply.starts_with(&format!("\x1b_Gi=1;{code}:")),
                "{reply:?}"
            );
            assert_eq!(t.images().placements().count(), 0);
            assert_eq!(t.images().refusals.slot, 0, "no slot was taken");
            // The whole arena is still free.
            feed(t, &apc("a=T,f=24,s=8,v=8,i=2", &b64(&[0u8; 192])), &mut r);
            assert_eq!(
                t.images().placements().count(),
                1,
                "the arena was untouched"
            );
        });
    }
}

#[test]
fn short_data_is_refused_and_extra_data_is_dropped_and_counted() {
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=T,f=24,s=2,v=2,i=1", &b64(&[1u8; 9])), &mut r);
        assert!(r.starts_with(b"\x1b_Gi=1;ENODATA:"));
        assert_eq!(t.images().placements().count(), 0);
        r.clear();
        feed(t, &apc("a=T,f=24,s=1,v=1,i=2", &b64(&[1u8; 6])), &mut r);
        assert_eq!(r, b"\x1b_Gi=2;OK\x1b\\");
        assert_eq!(t.kitty().excess, 3);
    });
}

#[test]
fn quiet_ids_and_numbers_shape_the_reply() {
    let px = b64(&[1u8, 2, 3]);
    let cases: [(&str, &[u8]); 7] = [
        ("a=t,f=24,s=1,v=1,i=4,q=1", b""),
        ("a=t,f=24,s=1,v=1,i=4,q=2", b""),
        ("a=t,f=24,s=1,v=1", b""),
        (
            "a=t,f=24,s=1,v=1,i=4,I=5",
            b"\x1b_Gi=4,I=5;EINVAL:Must not specify both image id and image number\x1b\\",
        ),
        ("a=t,f=24,s=1,v=1,I=13", b"\x1b_Gi=1,I=13;OK\x1b\\"),
        (
            "a=t,f=24,s=2,v=1,i=4,q=1",
            b"\x1b_Gi=4;ENODATA:Insufficient image data: 3 < 6\x1b\\",
        ),
        ("a=t,f=24,s=2,v=1,i=4,q=2", b""),
    ];
    for (ctrl, want) in cases {
        with(64, |t| {
            let mut r = Vec::new();
            feed(t, &apc(ctrl, &px), &mut r);
            assert_eq!(
                String::from_utf8_lossy(&r),
                String::from_utf8_lossy(want),
                "{ctrl}"
            );
        });
    }
}

#[test]
fn a_put_places_a_stored_image_and_a_second_put_with_its_ids_replaces_it() {
    with(256, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=t,f=24,s=1,v=1,i=3", &b64(&[1, 2, 3])), &mut r);
        assert_eq!(t.images().placements().count(), 0, "a=t places nothing");
        feed(t, &apc("a=p,i=3,p=1", b""), &mut r);
        feed(t, b"\x1b[10;10H", &mut r);
        feed(t, &apc("a=p,i=3,p=1,z=-5", b""), &mut r);
        feed(t, &apc("a=p,i=99", b""), &mut r);
        let r = String::from_utf8_lossy(&r).into_owned();
        assert_eq!(
            r,
            "\x1b_Gi=3;OK\x1b\\\x1b_Gi=3,p=1;OK\x1b\\\x1b_Gi=3,p=1;OK\x1b\\\
             \x1b_Gi=99;ENOENT:Put command refers to non-existent image\x1b\\"
        );
        let imgs = t.images();
        let all: Vec<_> = imgs.placements().map(|(_, p)| *p).collect();
        assert_eq!(all.len(), 1, "the second put replaced the first");
        assert_eq!(all[0].line, LineIdx::FIRST.plus(Rows::of_count(9)));
        assert_eq!(all[0].z.raw(), -5);
        assert_eq!(all[0].kitty.id, KittyPlacement::of(1));
    });
}

/// Three images placed at (1,1), (1,20) and (10,1), 2x2 cells each, ids 1,
/// 2 and 3, number 7 on the third, z 1, 2 and 3.
fn three(t: &mut Term<'_>, r: &mut Vec<u8>) {
    let px = b64(&[5u8; 20 * 40 * 3]);
    feed(t, &apc("a=T,f=24,s=20,v=40,i=1,z=1,C=1,q=2", &px), r);
    feed(t, b"\x1b[1;20H", r);
    feed(t, &apc("a=T,f=24,s=20,v=40,i=2,z=2,C=1,q=2", &px), r);
    feed(t, b"\x1b[10;1H", r);
    feed(t, &apc("a=T,f=24,s=20,v=40,I=7,z=3,C=1,q=2", &px), r);
}

/// The anchors still placed, as (row, column) from the top-left.
fn anchors(t: &Term<'_>) -> Vec<(usize, usize)> {
    let mut v: Vec<_> = t
        .images()
        .placements()
        .map(|(_, p)| {
            let row = p.line.since(LineIdx::FIRST).map_or(0, |r| r.index());
            (row, p.col.index())
        })
        .collect();
    v.sort();
    v
}

#[test]
fn every_delete_specifier_removes_what_it_names() {
    let all = vec![(0, 0), (0, 19), (9, 0)];
    let cases: [(&str, Vec<(usize, usize)>); 14] = [
        ("a=d", vec![]),
        ("a=d,d=a", vec![]),
        ("a=d,d=i,i=2", vec![(0, 0), (9, 0)]),
        ("a=d,d=n,I=7", vec![(0, 0), (0, 19)]),
        ("a=d,d=c", vec![(0, 19), (9, 0)]),
        ("a=d,d=p,x=21,y=2", vec![(0, 0), (9, 0)]),
        ("a=d,d=q,x=21,y=2,z=1", all.clone()),
        ("a=d,d=q,x=21,y=2,z=2", vec![(0, 0), (9, 0)]),
        ("a=d,d=r,x=1,y=2", vec![(9, 0)]),
        ("a=d,d=x,x=2", vec![(0, 19)]),
        ("a=d,d=y,y=11", vec![(0, 0), (0, 19)]),
        ("a=d,d=z,z=3", vec![(0, 0), (0, 19)]),
        ("a=d,d=f", all.clone()),
        ("a=d,d=Z,z=1", vec![(0, 19), (9, 0)]),
    ];
    for (cmd, left) in cases {
        with(4096, |t| {
            let mut r = Vec::new();
            three(t, &mut r);
            feed(t, b"\x1b[1;1H", &mut r);
            assert_eq!(anchors(t), all, "three placed");
            feed(t, &apc(cmd, b""), &mut r);
            assert_eq!(anchors(t), left, "{cmd}");
            assert!(r.is_empty(), "a delete answers nothing");
        });
    }
}

#[test]
fn lowercase_keeps_the_data_and_uppercase_frees_it() {
    with(4096, |t| {
        let mut r = Vec::new();
        three(t, &mut r);
        feed(t, &apc("a=d,d=i,i=1", b""), &mut r);
        feed(t, &apc("a=p,i=1", b""), &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\", "kept, and placed again");
        r.clear();
        feed(t, &apc("a=d,d=I,i=1", b""), &mut r);
        feed(t, &apc("a=p,i=1", b""), &mut r);
        assert!(r.starts_with(b"\x1b_Gi=1;ENOENT"), "freed");
        r.clear();
        // An image only stored is freed by `I` too.
        feed(
            t,
            &apc("a=t,f=24,s=1,v=1,i=8,q=2", &b64(&[1, 2, 3])),
            &mut r,
        );
        feed(t, &apc("a=d,d=I,i=8", b""), &mut r);
        feed(t, &apc("a=p,i=8", b""), &mut r);
        assert!(
            r.starts_with(b"\x1b_Gi=8;ENOENT"),
            "{:?}",
            String::from_utf8_lossy(&r)
        );
    });
}

#[test]
fn a_delete_aborts_an_upload_in_flight() {
    let data = b64(&[7u8; 12]);
    let mut s = apc("a=T,f=24,s=2,v=2,i=1,m=1", &data[..8]);
    s.extend(apc("a=d,d=A", b""));
    s.extend(apc("m=0", &data[8..]));
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        assert_eq!(t.images().placements().count(), 0, "nothing placed");
        // The arena is whole again.
        feed(t, &apc("a=T,f=24,s=8,v=8,i=2", &b64(&[0u8; 192])), &mut r);
        assert_eq!(t.images().placements().count(), 1);
    });
}

/// **A `CAN` at the end of a kitty chunk is its payload, and so is the
/// command after it, up to that command's `ST`**: kitty ends a command only
/// at `BEL` or `ESC \` (`find_st_terminator`, `[[source:
/// kitty-vt-parser-c]]`), so the second chunk runs on through `ESC _ G` and
/// ends at the next command's `ST`, its data ending at the `CAN` with the
/// first chunk's six octets and its own three: nine of twelve, refused
/// short. The command after it never runs, and nothing is cancelled. Under
/// Williams' rule the `CAN` cancelled the chunk and aborted its upload, and
/// was counted.
#[test]
fn a_can_ending_a_kitty_chunk_is_payload_up_to_the_next_st() {
    let data = b64(&[7u8; 12]);
    let mut s = apc("a=T,f=24,s=2,v=2,i=1,m=1", &data[..8]);
    s.extend_from_slice(b"\x1b_Gm=0;");
    s.extend_from_slice(&data[8..12]);
    s.push(0x18);
    s.extend(apc("a=T,f=24,s=1,v=1,i=2", &b64(&[1, 2, 3])));
    invariant(64, &s, Splits::Every);
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        assert_eq!(t.kitty().cancelled, 0);
        assert_eq!(
            String::from_utf8_lossy(&r),
            "\x1b_Gi=1;ENODATA:Insufficient image data: 9 < 12\x1b\\"
        );
        assert_eq!(t.images().placements().count(), 0);
    });
}

#[test]
fn text_does_not_erase_an_image_and_ed2_and_ris_do() {
    with(4096, |t| {
        let mut r = Vec::new();
        three(t, &mut r);
        feed(t, b"\x1b[1;1Hwritten over\x1b[K\x1b[J", &mut r);
        assert_eq!(anchors(t).len(), 3, "text and other erases leave images");
        feed(t, b"\x1b[2J", &mut r);
        assert!(anchors(t).is_empty(), "ED 2 clears them");
        three(t, &mut r);
        feed(t, b"\x1bc", &mut r);
        assert!(anchors(t).is_empty(), "RIS clears them");
        feed(t, &apc("a=p,i=1", b""), &mut r);
        assert!(r.starts_with(b"\x1b_Gi=1;ENOENT"), "and their data");
    });
}

/// A relative placement naming a placement the parent image does not have
/// is ENOPARENT, its reply the only change (`tests/kitty_relative.rs`
/// holds the rest); a frame is made and answered with its number
/// (`tests/kitty_animation.rs` holds the rest).
#[test]
fn a_relative_placement_naming_no_placement_is_refused_and_a_frame_made() {
    with(256, |t| {
        let mut r = Vec::new();
        feed(
            t,
            &apc("a=T,f=24,s=1,v=1,i=1,U=1", &b64(&[1, 2, 3])),
            &mut r,
        );
        feed(t, &apc("a=p,i=1,P=1,Q=1", b""), &mut r);
        feed(t, &apc("a=f,i=1,f=24,s=1,v=1", &b64(&[1, 2, 3])), &mut r);
        let r = String::from_utf8_lossy(&r).into_owned();
        // A virtual placement (`U=1`) is made and answered as a put is
        // (`tests/kitty_placeholder.rs`); a relative one to a placement id
        // it does not have is refused.
        assert_eq!(
            r,
            "\x1b_Gi=1;OK\x1b\\\
             \x1b_Gi=1;ENOPARENT:Put command refers to a parent image placement \
             that does not exist\x1b\\\
             \x1b_Gi=1,r=2;OK\x1b\\"
        );
        assert_eq!(t.images().placements().count(), 0);
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO), "no cursor move");
        assert_eq!(t.kitty().ignored, 0, "nothing ignored");
        assert_eq!(t.kitty().frames, 1, "the frame, made");
        // The image itself was stored by the a=T.
        let mut r = Vec::new();
        feed(t, &apc("a=p,i=1", b""), &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
    });
}

/// **A reply is `ESC _ G ... ESC \\` whatever S8C1T says**: the shape the
/// text gives ("`<ESC>_Gi=31;OK<ESC>\\`"). Following S8C1T is an open
/// question. The DA after it is the control: S8C1T took.
#[test]
fn replies_keep_the_seven_bit_shape_under_s8c1t() {
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b G", &mut r);
        feed(t, &apc("a=q,i=1,f=24,s=1,v=1", b"AAAA"), &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
        let mut da = Vec::new();
        feed(t, b"\x1b[c", &mut da);
        assert_eq!(da.first(), Some(&0x9b), "S8C1T is on: {da:?}");
    });
}

/// A 2x2 RGBA image, transmitted (`a=t`) or put (`a=T`), named `i`.
fn tiny(act: char, i: u32) -> Vec<u8> {
    apc(&format!("a={act},f=32,s=2,v=2,i={i}"), &b64(&bytes(16, i)))
}

/// Which of the ids `1..=n` name a live image: a quiet `a=p` of each,
/// answered `OK` or `ENOENT`, then deleted again with its data kept.
fn live(t: &mut Term<'_>, n: u32) -> Vec<u32> {
    (1..=n)
        .filter(|&i| {
            let mut r = Vec::new();
            feed(t, &apc(&format!("a=p,i={i},p=99,C=1"), b""), &mut r);
            feed(t, &apc(&format!("a=d,d=i,i={i},p=99,q=2"), b""), &mut r);
            r.starts_with(format!("\x1b_Gi={i},p=99;OK").as_bytes())
        })
        .collect()
}

/// **The quota, as the text gives it**: "When adding a new image, if the
/// total size exceeds the quota, the terminal emulator should delete older
/// images to make space for the new one", and "existing images without
/// placements will be preferentially deleted". Three 2x2 images fill a
/// 12-pixel arena; a fourth deletes the oldest, and only it.
#[test]
fn a_new_image_deletes_the_oldest_unplaced_image_to_make_space() {
    let mut s = Vec::new();
    for i in 1..=4 {
        s.extend(tiny('t', i));
    }
    invariant(12, &s, Splits::Every);
    with(12, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        assert!(r.ends_with(b"\x1b_Gi=4;OK\x1b\\"), "{r:?}");
        let imgs = t.images();
        assert_eq!((imgs.images_evicted, imgs.images_refused), (1, 0));
        assert_eq!(live(t, 4), [2, 3, 4], "the oldest went");
    });
}

/// **Once no image without a placement is left, the oldest with one
/// goes**: "existing images without placements will be preferentially
/// deleted", and "the terminal emulator should delete older images to make
/// space for the new one" (`[[source: kitty-graphics]]`). A 12-pixel arena
/// holds three 2x2 images: 1 and 2 placed, 3 not. Image 4 deletes 3, though
/// 1 and 2 are older; 5, placed, deletes 4; 6 finds none unplaced and
/// deletes 1, the oldest placed, and its placement with it; 7 deletes 6,
/// unplaced, before the placed 2.
#[test]
fn once_no_unplaced_image_is_left_the_oldest_placed_image_goes() {
    with(12, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('T', 1), &mut r);
        feed(t, &tiny('T', 2), &mut r);
        feed(t, &tiny('t', 3), &mut r);
        let steps: [(char, u32, [u32; 3], usize); 4] = [
            ('t', 4, [1, 2, 4], 2),
            ('T', 5, [1, 2, 5], 3),
            ('t', 6, [2, 5, 6], 2),
            ('t', 7, [2, 5, 7], 2),
        ];
        for (n, (act, i, alive, placed)) in steps.into_iter().enumerate() {
            r.clear();
            feed(t, &tiny(act, i), &mut r);
            assert!(
                r.ends_with(format!("\x1b_Gi={i};OK\x1b\\").as_bytes()),
                "{r:?}"
            );
            assert_eq!(t.images().images_evicted, n + 1, "image {i}: one more went");
            assert_eq!(t.images().placements().count(), placed, "image {i}");
            assert_eq!(live(t, 7), alive, "image {i}");
        }
        assert_eq!(t.images().images_refused, 0);
    });
}

/// **When deleting every kitty image would not make space, none is
/// deleted**: the new image is refused with ENOSPC and counted once. A
/// Sixel is never deleted for space, so one between two placed kitty
/// images leaves gaps of 4 pixels however many go, none the 6 a 3x2
/// image needs. The controls: two placed images in an 8-pixel arena, and
/// a placed, an unplaced and a placed one in 14, each make way for the
/// new image, the unplaced first.
#[test]
fn with_nothing_to_delete_the_new_image_is_refused_and_counted_once() {
    with(14, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('T', 1), &mut r);
        feed(t, b"\x1bP0;1;0q\"1;1#1;2;100;0;0#1~\x1b\\", &mut r);
        feed(t, &tiny('T', 2), &mut r);
        assert_eq!(t.images().placements().count(), 3, "the premise");
        r.clear();
        let wide = apc("a=t,f=32,s=3,v=2,i=4", &b64(&bytes(24, 4)));
        feed(t, &wide, &mut r);
        assert!(r.starts_with(b"\x1b_Gi=4;ENOSPC:"), "{r:?}");
        let imgs = t.images();
        assert_eq!((imgs.images_evicted, imgs.images_refused), (0, 1));
        assert_eq!(imgs.placements().count(), 3, "nothing deleted for nothing");
    });
    with(8, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('T', 1), &mut r);
        feed(t, &tiny('T', 2), &mut r);
        r.clear();
        feed(t, &tiny('t', 3), &mut r);
        assert!(r.starts_with(b"\x1b_Gi=3;OK"), "{r:?}");
        let imgs = t.images();
        assert_eq!((imgs.images_evicted, imgs.images_refused), (1, 0));
        assert_eq!(imgs.placements().count(), 1);
        assert_eq!(live(t, 3), [2, 3], "the oldest placed went");
    });
    with(14, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('T', 1), &mut r);
        feed(t, &tiny('t', 2), &mut r);
        feed(t, &tiny('T', 3), &mut r);
        r.clear();
        let wide = apc("a=t,f=32,s=3,v=2,i=4", &b64(&bytes(24, 4)));
        feed(t, &wide, &mut r);
        assert!(r.starts_with(b"\x1b_Gi=4;OK"), "{r:?}");
        assert_eq!(
            t.images().images_evicted,
            2,
            "the unplaced, then the oldest"
        );
        assert_eq!(live(t, 4), [3, 4]);
    });
}

/// **The quota keeps what the command needs**: a parent the new image
/// is put against, and an image a frame is added to, with every image
/// their placements hang under -- deleting a parent deletes the placements
/// under it, "If its parent is deleted, it is deleted as well"
/// (`[[source: kitty-graphics]]`, "Relative placements"). Three placed
/// 2x2 images fill 12 pixels; image 4, put against image 1, deletes image
/// 2, the oldest the command does not need, and is placed under image 1.
#[test]
fn the_quota_keeps_the_parent_a_new_image_is_put_against() {
    with(12, |t| {
        let mut r = Vec::new();
        for i in 1..=3 {
            feed(t, &tiny('T', i), &mut r);
        }
        r.clear();
        let put = apc("a=T,f=32,s=2,v=2,i=4,P=1", &b64(&bytes(16, 4)));
        feed(t, &put, &mut r);
        assert_eq!(r, b"\x1b_Gi=4;OK\x1b\\", "{}", String::from_utf8_lossy(&r));
        assert_eq!(t.images().images_evicted, 1);
        assert_eq!(t.images().placements().count(), 3, "1, 3, and 4 under 1");
        assert_eq!(live(t, 4), [1, 3, 4]);
    });
}

/// **And the image a frame is added to**, with the image its placement
/// hangs under: image 2 is placed against image 1, the oldest; image 3
/// and image 4 are placed beside them in a 16-pixel arena. A frame for
/// image 2 deletes image 3, not image 1, whose delete would take image 2's
/// placement and then image 2 with it.
#[test]
fn the_quota_keeps_the_image_a_frame_is_added_to_and_its_parent() {
    with(16, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('T', 1), &mut r);
        feed(t, &tiny('t', 2), &mut r);
        feed(t, &apc("a=p,i=2,P=1,q=2", b""), &mut r);
        feed(t, &tiny('T', 3), &mut r);
        feed(t, &tiny('T', 4), &mut r);
        assert_eq!(t.images().placements().count(), 4, "the premise");
        r.clear();
        feed(t, &apc("a=f,f=32,s=2,v=2,i=2", &b64(&bytes(16, 9))), &mut r);
        let alive = live(t, 4);
        assert!(
            alive.contains(&1) && alive.contains(&2),
            "{alive:?}: {}",
            String::from_utf8_lossy(&r)
        );
        assert!(!alive.contains(&3), "{alive:?}: the oldest not needed went");
    });
}

/// What a query must leave as it found it: the images, the counts and the
/// epoch a texture cache keys on.
fn untouched(t: &Term<'_>) -> (usize, usize, usize, u32) {
    let imgs = t.images();
    (
        imgs.images_evicted,
        imgs.images_refused,
        imgs.images_dropped,
        imgs.generation().raw(),
    )
}

/// **A query is answered on a full arena, and deletes nothing**: it "will
/// try to load the image and respond with either OK or an error ... but it
/// will not replace an existing image with the same id, nor will it store
/// the image" (`[[source: kitty-graphics]]`, "Querying support and
/// available transmission mediums"). It decodes in the store's probe,
/// outside the arena and its quota, in every format and compression; the
/// probe is released after each, so the next query is answered too.
#[test]
fn a_query_on_a_full_arena_is_answered_and_deletes_nothing() {
    let rgba = bytes(16, 9);
    let rgb = bytes(12, 9);
    let queries = [
        apc("a=q,i=9,f=32,s=2,v=2", &b64(&rgba)),
        apc("a=q,i=9,f=24,s=2,v=2", &b64(&rgb)),
        apc("a=q,i=9,f=32,s=2,v=2,o=z", &b64(&zlib_mixed(&rgba))),
        apc("a=q,i=9,f=32,s=4,v=4", &b64(&bytes(64, 9))),
    ];
    for q in &queries {
        with(8, |t| {
            let mut r = Vec::new();
            feed(t, &tiny('t', 1), &mut r);
            feed(t, &tiny('t', 2), &mut r);
            let before = untouched(t);
            r.clear();
            feed(t, q, &mut r);
            feed(t, q, &mut r);
            assert_eq!(r, b"\x1b_Gi=9;OK\x1b\\\x1b_Gi=9;OK\x1b\\", "{r:?}");
            assert_eq!(untouched(t), before, "nothing evicted, stored or freed");
            assert_eq!(live(t, 2), [1, 2]);
        });
    }
    // A PNG, on an arena whose scratch holds one, full of an unplaced image.
    let p = png(2, 2, png::ColorType::Rgba, &bytes(16, 3), 8);
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=t,f=24,s=8,v=8,i=1", &b64(&[7u8; 192])), &mut r);
        let before = untouched(t);
        r.clear();
        feed(t, &apc("a=q,i=7,f=100", &b64(&p)), &mut r);
        assert_eq!(
            r,
            b"\x1b_Gi=7;OK\x1b\\",
            "{:?}",
            String::from_utf8_lossy(&r)
        );
        assert_eq!(untouched(t), before);
        assert_eq!(live(t, 1), [1]);
    });
}

/// **A query in the probe touches nothing**, on an arena with room too: no
/// slot, no epoch, no count, and no trim of an image nothing names (`a=t`
/// with no `i`), which a transmission would delete.
#[test]
fn a_query_with_room_in_the_arena_still_touches_nothing() {
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=t,f=32,s=2,v=2", &b64(&bytes(16, 5))), &mut r);
        let before = untouched(t);
        feed(t, &apc("a=q,i=9,f=32,s=2,v=2", &b64(&bytes(16, 9))), &mut r);
        assert_eq!(r, b"\x1b_Gi=9;OK\x1b\\", "{r:?}");
        assert_eq!(
            untouched(t),
            before,
            "the nameless image stays, the epoch too"
        );
    });
}

/// A PNG query larger than the arena and within the probe is answered:
/// its `IHDR` is checked against the probe, not only the arena. 9 x 9
/// pixels against a 64-pixel arena whose 512-byte scratch holds the file
/// and its scanlines.
#[test]
fn a_png_query_within_the_probe_is_answered_on_a_smaller_arena() {
    let p = png(9, 9, png::ColorType::Rgba, &[0u8; 324], 64);
    assert!(p.len() + 9 * 37 <= 512, "{} bytes", p.len());
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=q,i=7,f=100", &b64(&p)), &mut r);
        assert_eq!(
            r,
            b"\x1b_Gi=7;OK\x1b\\",
            "{:?}",
            String::from_utf8_lossy(&r)
        );
    });
}

/// A short or broken query in the probe is still its error, and releases
/// the probe for the next.
#[test]
fn a_short_or_broken_query_is_its_error_and_frees_the_probe() {
    with(8, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('t', 1), &mut r);
        feed(t, &tiny('t', 2), &mut r);
        r.clear();
        feed(t, &apc("a=q,i=9,f=32,s=2,v=2", &b64(&bytes(12, 9))), &mut r);
        assert!(r.starts_with(b"\x1b_Gi=9;ENODATA:"), "{r:?}");
        r.clear();
        feed(t, &apc("a=q,i=9,f=32,s=2,v=2", &b64(&bytes(16, 9))), &mut r);
        assert_eq!(r, b"\x1b_Gi=9;OK\x1b\\", "the probe was released");
        assert_eq!(live(t, 2), [1, 2]);
        // Data past `s` x `v` is dropped and counted in the probe as in the
        // arena: the probe lends the query its size, not all of itself.
        let excess = t.kitty().excess;
        r.clear();
        feed(t, &apc("a=q,i=9,f=32,s=2,v=2", &b64(&bytes(20, 9))), &mut r);
        assert_eq!(r, b"\x1b_Gi=9;OK\x1b\\");
        assert_eq!(t.kitty().excess - excess, 4, "one pixel past the size");
    });
    let mut bad = png(2, 2, png::ColorType::Rgba, &bytes(16, 3), 8);
    bad[40] ^= 0xFF;
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=t,f=24,s=8,v=8,i=1", &b64(&[7u8; 192])), &mut r);
        r.clear();
        feed(t, &apc("a=q,i=8,f=100", &b64(&bad)), &mut r);
        assert!(r.starts_with(b"\x1b_Gi=8;EBADPNG:"), "{r:?}");
        r.clear();
        feed(t, &apc("a=q,i=9,f=32,s=2,v=2", &b64(&bytes(16, 9))), &mut r);
        assert_eq!(r, b"\x1b_Gi=9;OK\x1b\\", "the probe was released");
    });
}

/// **A query larger than the probe** decodes in a gap of the arena when one
/// holds it, deleting nothing either way, and is answered ENOSPC when none
/// does: 100 x 50 pixels against a 64 x 64 probe.
#[test]
fn a_query_larger_than_the_probe_uses_a_gap_or_is_refused() {
    let big = apc("a=q,i=9,f=24,s=100,v=50", &b64(&bytes(15_000, 9)));
    with(10_000, |t| {
        let mut r = Vec::new();
        feed(t, &big, &mut r);
        assert_eq!(r, b"\x1b_Gi=9;OK\x1b\\", "{r:?}");
        r.clear();
        feed(
            t,
            &apc("a=t,f=24,s=100,v=100,i=1", &b64(&bytes(30_000, 1))),
            &mut r,
        );
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\", "the query left the arena free");
    });
    with(10_000, |t| {
        let mut r = Vec::new();
        feed(
            t,
            &apc("a=t,f=24,s=100,v=100,i=1", &b64(&bytes(30_000, 1))),
            &mut r,
        );
        let (evicted, _, dropped, _) = untouched(t);
        r.clear();
        feed(t, &big, &mut r);
        assert!(r.starts_with(b"\x1b_Gi=9;ENOSPC:"), "{r:?}");
        assert_eq!((untouched(t).0, untouched(t).2), (evicted, dropped));
        assert_eq!(live(t, 1), [1], "nothing deleted for a query");
    });
}

/// A full arena, two queries and an image after them: the same however the
/// reads fall.
#[test]
fn queries_on_a_full_arena_are_chunk_invariant() {
    let mut s = tiny('t', 1);
    s.extend(tiny('t', 2));
    s.extend(apc("a=q,i=9,f=32,s=2,v=2", &b64(&bytes(16, 9))));
    s.extend(apc(
        "a=q,i=10,f=24,s=2,v=2,o=z",
        &b64(&zlib_mixed(&bytes(12, 4))),
    ));
    s.extend(tiny('T', 3));
    invariant(8, &s, Splits::Every);
}

#[test]
fn an_unprovisioned_terminal_answers_a_query_with_an_error() {
    let mut owned = Owned::new(&WS);
    let mut t = Term::new(owned.storage(), &WS);
    let mut r = Vec::new();
    feed(&mut t, &apc("a=q,i=1,f=24,s=1,v=1", b"AAAA"), &mut r);
    assert!(
        r.starts_with(b"\x1b_Gi=1;EINVAL:Image too large"),
        "{:?}",
        String::from_utf8_lossy(&r)
    );
}

/// A red Sixel at the cursor: two columns, six rows at the 2:1 default
/// aspect, so twelve pixels high; `P2` 1, so no background fill reaches
/// past its data.
const SIXEL: &[u8] = b"\x1bP0;1q#0;2;100;0;0#0~~\x1b\\";

/// The images visible now, as their sizes.
fn visible(t: &Term<'_>) -> Vec<(u16, u16)> {
    let imgs = t.images();
    imgs.placements()
        .filter_map(|(_, p)| imgs.image(p.image))
        .map(|v| (v.width.raw(), v.height.raw()))
        .collect()
}

/// **An image cleared before its decode completes takes nothing with it**:
/// `ED 2` frees a decoding image, a Sixel takes its slot, and the decode's
/// next step must neither free the Sixel nor answer as if the PNG were bad.
#[test]
fn a_decode_whose_image_was_cleared_frees_nothing_else() {
    let p = png(21, 23, png::ColorType::Rgba, &bytes(21 * 23 * 4, 91), 50);
    with(4096, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=T,f=100,i=7", &b64(&p)), &mut r);
        feed(t, b"\x1b[2J\x1b[H", &mut r);
        feed(t, SIXEL, &mut r);
        settle(t, &mut r);
        assert_eq!(visible(t), vec![(2, 12)], "the Sixel survives");
        assert_eq!(
            String::from_utf8_lossy(&r),
            "\x1b_Gi=7;ENOENT:Image deleted before its decode completed\x1b\\"
        );
    });
    // An embedder's image in the slot instead.
    with(4096, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=T,f=100,i=7", &b64(&p)), &mut r);
        feed(t, b"\x1b[2J", &mut r);
        let placed = t.put_image(
            ImagePx::of(1),
            ImagePx::of(1),
            ImageAlpha::OpaqueOrClear,
            |px| px[0] = Pixel::CLEAR,
        );
        assert!(!placed.is_none());
        settle(t, &mut r);
        assert_eq!(visible(t), vec![(1, 1)], "the embedder's image survives");
    });
    // A nameless image whose screen is left goes with it, the same way.
    with(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[?1049h", &mut r);
        feed(t, &apc("a=T,f=100", &b64(&p)), &mut r);
        feed(t, b"\x1b[?1049l", &mut r);
        feed(t, SIXEL, &mut r);
        settle(t, &mut r);
        assert_eq!(visible(t), vec![(2, 12)], "the Sixel survives");
        assert!(r.is_empty(), "a nameless command is not answered");
    });
}

/// The lines of the kitty placements on screen.
fn kitty_lines(t: &Term<'_>) -> Vec<u64> {
    t.images()
        .placements()
        .map(|(_, p)| p.line.value())
        .collect()
}

/// A 10 by 40 image, two rows of the 10x20 cell, placed at `row`
/// (one-based) under the margins 5 to 10, the cursor left on the
/// region's bottom row; then `scroll`. Returns the placements' lines, and
/// how many are left after a delete of those on row `probe` (one-based),
/// which says which rows a placement still covers.
fn in_margins(row: u32, scroll: &[u8], probe: u32) -> (Vec<u64>, usize) {
    with(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[5;10r", &mut r);
        feed(t, format!("\x1b[{row};1H").as_bytes(), &mut r);
        feed(
            t,
            &apc("a=T,f=24,s=10,v=40,i=1,q=2,C=1", &b64(&[7u8; 1200])),
            &mut r,
        );
        feed(t, b"\x1b[10;1H", &mut r);
        feed(t, scroll, &mut r);
        let lines = kitty_lines(t);
        feed(t, &apc(&format!("a=d,d=y,y={probe},q=2"), b""), &mut r);
        (lines, t.images().placements().count())
    })
}

/// "When page margins are defined and the index commands are used, only
/// images that are entirely within the page area (between the margins)
/// must be scrolled. When scrolling them would cause them to extend
/// outside the page area, they must be clipped" (`[[source:
/// kitty-graphics]]`, "Interaction with other terminal actions"). Which
/// rows a placement still covers is read by a row delete, `d=y`.
#[test]
fn a_margin_scroll_moves_only_an_image_inside_the_margins_and_clips_it() {
    // Inside, rows 8 and 9 (lines 7 and 8): one LF at the bottom margin
    // moves it up a row, both rows kept, so a delete on its second row
    // (row 8) takes it.
    assert_eq!(in_margins(8, b"\n", 8), (vec![6], 0), "moved, uncut");
    // At the top margin, rows 5 and 6: moved up a row, its first row cut
    // and its line the margin's; a delete on row 4, where its cut row
    // would be, leaves it.
    assert_eq!(in_margins(5, b"\n", 4), (vec![4], 1), "the top row cut");
    // Two rows: nothing of it is left inside, so it goes.
    assert_eq!(in_margins(5, b"\n\n", 5), (vec![], 0), "scrolled out");
    // Across the bottom margin, rows 10 and 11: not wholly inside, so it
    // stays where it is.
    assert_eq!(in_margins(10, b"\n", 10), (vec![9], 0), "across a margin");
    // Down, by `SD`, from rows 9 and 10: moved down a row, its last row
    // cut, so a delete on row 11 leaves it.
    assert_eq!(
        in_margins(9, b"\x1b[T", 11),
        (vec![9], 1),
        "the bottom row cut"
    );
}

/// [`in_margins`] under side margins too, columns 2 to 40 (`DECLRMM`,
/// `DECSLRM`): a 10 by 40 image placed at `row` and `col` with `keys`, the
/// scroll then a rectangle's.
fn in_side_margins(row: u32, col: u32, keys: &str, scroll: &[u8], probe: u32) -> (Vec<u64>, usize) {
    with(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[?69h\x1b[5;10r\x1b[2;40s", &mut r);
        feed(t, format!("\x1b[{row};{col}H").as_bytes(), &mut r);
        let ctrl = format!("a=T,f=24,s=10,v=40,i=1,q=2,C=1{keys}");
        feed(t, &apc(&ctrl, &b64(&[7u8; 1200])), &mut r);
        feed(t, b"\x1b[10;3H", &mut r);
        feed(t, scroll, &mut r);
        let lines = kitty_lines(t);
        feed(t, &apc(&format!("a=d,d=y,y={probe},q=2"), b""), &mut r);
        (lines, t.images().placements().count())
    })
}

/// Under side margins the page area is a rectangle, so "entirely within
/// the page area (between the margins)" is its columns as well as its
/// rows (`[[source: kitty-graphics]]`, "Interaction with other terminal
/// actions"): the rectangle's scroll moves and clips an image wholly
/// inside it as the full-width scroll does, and leaves one across any of
/// its margins where it is.
#[test]
fn a_rectangle_scroll_moves_only_an_image_inside_its_margins_and_clips_it() {
    assert_eq!(
        in_side_margins(8, 3, "", b"\n", 8),
        (vec![6], 0),
        "moved, uncut"
    );
    assert_eq!(
        in_side_margins(5, 3, "", b"\n", 4),
        (vec![4], 1),
        "the top row cut"
    );
    assert_eq!(
        in_side_margins(10, 3, "", b"\n", 10),
        (vec![9], 0),
        "across the bottom margin"
    );
    // `c=2` alone makes it four rows: rows 6 to 9, inside the rows, and
    // columns 40 and 41, across the right margin, so it is left where it is.
    assert_eq!(
        in_side_margins(6, 40, ",c=2", b"\n", 6),
        (vec![5], 0),
        "across the right margin"
    );
    // The control: columns 38 and 39, inside, scrolled up a row.
    assert_eq!(
        in_side_margins(6, 38, ",c=2", b"\n", 6),
        (vec![4], 0),
        "inside, scrolled"
    );
}

/// A terminal with `pixels` of arena, a 10x20 cell and six lines of
/// history, so a picture reaches the history's far end in a few lines.
fn with_history<R>(pixels: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::with_history(&WS, &CacheGeometry::UNKNOWN, Depth::of(6))
        .with_images(ImageBudget::of(pixels));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    f(&mut t)
}

/// The oldest line the primary screen retains.
fn oldest(t: &Term<'_>) -> u64 {
    let hist = u64::try_from(t.scrollback().len()).unwrap();
    t.image_origin(Rows::ZERO).top.value() - hist
}

/// Line feeds at the bottom row until the oldest retained line is `line`.
fn scroll_until(t: &mut Term<'_>, line: u64, r: &mut Vec<u8>) {
    feed(t, b"\x1b[24;1H", r);
    while oldest(t) < line {
        feed(t, b"\n", r);
    }
    assert_eq!(oldest(t), line, "the premise: scrolled exactly there");
}

/// **A tall picture goes when its last row leaves the history, not its
/// first**: images "must be scrolled along with text"
/// (`[[source: kitty-graphics]]`, "Interaction with other terminal
/// actions"), and the rows of a placement below its anchor are over text
/// the history still holds. Kitty drops a placement when
/// `start_row + effective_num_rows <= limit`, the limit the history's far
/// end (`[[source: kitty-graphics-c]]` `scroll_filter_func`,
/// `[[source: kitty-screen-c]]` `INDEX_GRAPHICS`). A four-row picture on
/// line 2 is kept with its anchor gone, kept with only its last row
/// retained, and gone, counted, once that row goes.
#[test]
fn a_tall_picture_goes_when_its_last_row_leaves_the_history() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[3;1H", &mut r);
        feed(
            t,
            &apc("a=T,f=24,s=10,v=40,i=1,r=4,q=2,C=1", &b64(&[7u8; 1200])),
            &mut r,
        );
        assert_eq!(kitty_lines(t), vec![2], "the premise: anchored on line 2");
        scroll_until(t, 3, &mut r);
        assert_eq!(kitty_lines(t), vec![2], "its anchor gone, three rows kept");
        scroll_until(t, 5, &mut r);
        assert_eq!(kitty_lines(t), vec![2], "its last row kept");
        assert_eq!(t.images().placements_expired, 0);
        scroll_until(t, 6, &mut r);
        assert_eq!(kitty_lines(t), Vec::<u64>::new(), "its last row gone");
        assert_eq!(t.images().placements_expired, 1, "and counted");
    });
}

/// The rows from the screen's top to each placement's anchor, negative
/// above it, and how many placements expired.
fn shown_rows(t: &Term<'_>) -> (Vec<i64>, usize) {
    let top = i64::try_from(t.image_origin(Rows::ZERO).top.value()).unwrap();
    let rows = kitty_lines(t)
        .into_iter()
        .map(|l| i64::try_from(l).unwrap() - top)
        .collect();
    (rows, t.images().placements_expired)
}

/// A picture `rows` tall placed on the top row, then one line feed at
/// the bottom, so its anchor is the newest line of the history; then
/// `scroll` with the cursor on the top row. Returns the rows before and
/// after.
fn taken_back(alt: bool, rows: u32, scroll: &[u8]) -> ((Vec<i64>, usize), (Vec<i64>, usize)) {
    with_history(1 << 16, |t| {
        let mut r = Vec::new();
        if alt {
            feed(t, b"\x1b[?1049h", &mut r);
        }
        let ctrl = format!("a=T,f=24,s=10,v={},i=1,q=2,C=1", rows * 20);
        let px = b64(&vec![7u8; usize::try_from(rows).unwrap() * 600]);
        feed(t, b"\x1b[1;1H", &mut r);
        feed(t, &apc(&ctrl, &px), &mut r);
        feed(t, b"\x1b[24;1H\n\x1b[1;1H", &mut r);
        let before = shown_rows(t);
        feed(t, scroll, &mut r);
        (before, shown_rows(t))
    })
}

/// **A reverse scroll moves a picture down with its text, and drops
/// none**: images "must be scrolled along with text" (`[[source:
/// kitty-graphics]]`, "Interaction with other terminal actions"), and
/// kitty's `INDEX_DOWN` moves every image down a row (`[[source:
/// kitty-screen-c]]`, `[[source: kitty-graphics-c]]` `scroll_filter_func`
/// with `amt` 1). A full-screen `RI` or `SD` takes the newest line of
/// the history back as the blank top row, so a picture anchored there
/// is drawn on that row, a tall one with its lower rows one row down
/// over the text that moved: where kitty draws both. It went with the
/// line before, the tall one's rows on screen with it.
#[test]
fn a_reverse_scroll_moves_a_picture_on_the_line_it_takes_back_down() {
    for scroll in [&b"\x1bM"[..], b"\x1b[T"] {
        for (alt, rows) in [(false, 1), (false, 4), (true, 4)] {
            let (before, after) = taken_back(alt, rows, scroll);
            assert_eq!(
                before,
                (vec![-1], 0),
                "the premise: one row above the screen"
            );
            assert_eq!(after, (vec![0], 0), "alt {alt}, {rows} rows, {scroll:?}");
        }
    }
}

/// A picture on the top row a reverse scroll moves down is one row
/// down after it, on either screen and with or without a history.
#[test]
fn a_reverse_scroll_moves_a_picture_on_the_top_row_one_row_down() {
    for alt in [false, true] {
        for scroll in [&b"\x1bM"[..], b"\x1b[T"] {
            let after = with(4096, |t| {
                let mut r = Vec::new();
                if alt {
                    feed(t, b"\x1b[?1049h", &mut r);
                }
                feed(t, b"\x1b[1;1H", &mut r);
                feed(
                    t,
                    &apc("a=T,f=24,s=10,v=20,i=1,q=2,C=1", &b64(&[7u8; 600])),
                    &mut r,
                );
                feed(t, b"\x1b[1;1H", &mut r);
                feed(t, scroll, &mut r);
                shown_rows(t)
            });
            assert_eq!(after, (vec![1], 0), "alt {alt}, {scroll:?}");
        }
    }
}

/// A one-row picture, image 1 placement 7, on the bottom row of the
/// screen `enter` brings up, the primary for none; then `RI` with the
/// cursor on the top row, which pushes it below the screen.
fn push_below(t: &mut Term<'_>, enter: &[u8], r: &mut Vec<u8>) {
    feed(t, enter, r);
    feed(t, b"\x1b[24;1H", r);
    feed(
        t,
        &apc("a=T,f=24,s=10,v=20,i=1,p=7,q=2,C=1", &b64(&[7u8; 600])),
        r,
    );
    feed(t, b"\x1b[1;1H\x1bM", r);
}

/// **A picture a reverse scroll pushes below the screen comes back with
/// its row, on either screen**: a line feed at the bottom brings the row
/// back blank and the picture drawn over it, as kitty draws it. Its
/// `INDEX_DOWN` moves every image down a row, and its
/// `scroll_filter_func` drops one only once its last row is above the
/// limit, the history's far end on the primary and the screen's top on
/// the alternate screen, and never for being below the screen, so
/// `INDEX_UP` moves it back (`[[source: kitty-screen-c]]`
/// `INDEX_GRAPHICS`, `[[source: kitty-graphics-c]]`). The primary keeps
/// the row under its window; the alternate screen keeps no row there,
/// and keeps the picture all the same.
#[test]
fn a_picture_pushed_below_the_screen_comes_back_with_its_row() {
    for enter in [&b""[..], b"\x1b[?1049h", b"\x1b[?47h"] {
        let (under, back) = with_history(4096, |t| {
            let mut r = Vec::new();
            push_below(t, enter, &mut r);
            let under = shown_rows(t);
            feed(t, b"\x1b[24;1H\n", &mut r);
            (under, shown_rows(t))
        });
        assert_eq!(
            under,
            (vec![24], 0),
            "{:?}: below the screen, kept",
            String::from_utf8_lossy(enter)
        );
        assert_eq!(
            back,
            (vec![23], 0),
            "{:?}: back on the bottom row",
            String::from_utf8_lossy(enter)
        );
    }
}

/// **Far below the alternate screen, and back a row a line feed**: the
/// first three reverse scrolls move the window back over lines its
/// numbering has, the rest go past its first line, where the pictures
/// move instead (`ImageStore::shift`); `SD 3` is three of them. Each line
/// feed at the bottom then brings the picture one row nearer, as kitty's
/// `start_row` steps by one each way.
#[test]
fn a_picture_far_below_the_alternate_screen_comes_back_a_row_a_line_feed() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[?1049h\x1b[24;1H", &mut r);
        feed(
            t,
            &apc("a=T,f=24,s=10,v=20,i=1,q=2,C=1", &b64(&[7u8; 600])),
            &mut r,
        );
        feed(t, b"\n\n\n", &mut r);
        assert_eq!(shown_rows(t), (vec![20], 0), "the premise: up three rows");
        feed(t, b"\x1b[1;1H\x1bM\x1bM\x1bM\x1bM", &mut r);
        assert_eq!(shown_rows(t), (vec![24], 0), "one row below the screen");
        feed(t, b"\x1b[3T", &mut r);
        assert_eq!(shown_rows(t), (vec![27], 0), "four rows below");
        feed(t, b"\x1b[24;1H\n\n\n", &mut r);
        assert_eq!(shown_rows(t), (vec![24], 0), "one row below again");
        feed(t, b"\n", &mut r);
        assert_eq!(shown_rows(t), (vec![23], 0), "back on the bottom row");
    });
}

/// **A scroll inside margins leaves a picture below the alternate screen
/// where it is**: "When page margins are defined and the index commands
/// are used, only images that are entirely within the page area (between
/// the margins) must be scrolled" (`[[source: kitty-graphics]]`,
/// "Interaction with other terminal actions"), and a picture below the
/// screen is not in it; kitty's `scroll_filter_margins_func()` leaves it
/// (`[[source: kitty-graphics-c]]`). The region is a status line's,
/// `CSI 1 ; 23 r`, which scrolls by moving the window; one row below and
/// two, by a line feed and by `SU 3`. A full-screen line feed at the
/// bottom then brings it a row nearer.
#[test]
fn a_region_scroll_leaves_a_picture_below_the_alternate_screen() {
    for (extra, scroll, n) in [
        (0, &b"\n"[..], 1),
        (1, b"\n", 1),
        (0, b"\x1b[3S", 3),
        (1, b"\x1b[3S", 3),
    ] {
        with_history(4096, |t| {
            let mut r = Vec::new();
            push_below(t, b"\x1b[?1049h", &mut r);
            for _ in 0..extra {
                feed(t, b"\x1bM", &mut r);
            }
            let below = 24 + extra;
            let at = format!("{extra} more below, {:?}", String::from_utf8_lossy(scroll));
            assert_eq!(shown_rows(t), (vec![below], 0), "{at}: the premise");
            let top = t.image_origin(Rows::ZERO).top.value();
            feed(t, b"\x1b[1;23r\x1b[23;1H", &mut r);
            feed(t, scroll, &mut r);
            feed(t, b"\x1b[r", &mut r);
            assert_eq!(
                t.image_origin(Rows::ZERO).top.value() - top,
                n,
                "{at}: the premise, the region moved the window"
            );
            assert_eq!(shown_rows(t), (vec![below], 0), "{at}: left below");
            feed(t, b"\x1b[24;1H\n", &mut r);
            assert_eq!(shown_rows(t), (vec![below - 1], 0), "{at}: a row nearer");
        });
    }
}

/// **`ED 2` takes a picture below the alternate screen too**: "The clear
/// screen escape code (usually <ESC>[2J) should also clear all images"
/// (`[[source: kitty-graphics]]`, "Interaction with other terminal
/// actions"), and kitty's `ED 2` calls `grman_clear`, which takes every
/// placement but one wholly above the screen (`[[source: kitty-screen-c]]`
/// `screen_erase_in_display()`, `[[source: kitty-graphics-c]]`
/// `clear_filter_func`, `start_row + effective_num_rows > 0`). A clear
/// that left it would let the next line feed at the bottom bring a
/// picture back over the cleared screen.
#[test]
fn clearing_the_screen_takes_a_picture_below_the_alternate_screen() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        push_below(t, b"\x1b[?1049h", &mut r);
        assert_eq!(shown_rows(t), (vec![24], 0), "the premise");
        feed(t, b"\x1b[2J\x1b[24;1H\n", &mut r);
        assert_eq!(shown_rows(t), (vec![], 0), "cleared");
    });
}

/// **The primary's `ED 2` leaves a picture under its window with its
/// row**: the row a reverse scroll pushed under the window keeps its
/// text, which `ED 2` erases only on the screen's rows, and a taller
/// window shows it again, the picture beside it, as an image follows the
/// row it was placed on.
#[test]
fn clearing_the_primary_leaves_a_picture_under_its_window_with_its_row() {
    let room = WinSize {
        ws_row: 25,
        ws_col: 80,
    };
    let mut owned = Owned::with_history(&room, &CacheGeometry::UNKNOWN, Depth::of(6))
        .with_images(ImageBudget::of(4096));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    let t = &mut t;
    let mut r = Vec::new();
    feed(t, b"\x1b[24;1H\n\x1b[24;1HMARKER\x1b[24;20H", &mut r);
    feed(
        t,
        &apc("a=T,f=24,s=10,v=20,i=1,q=2,C=1", &b64(&[7u8; 600])),
        &mut r,
    );
    feed(t, b"\x1b[1;1H\x1bM\x1b[2J", &mut r);
    assert_eq!(shown_rows(t), (vec![24], 0), "kept under the window");
    assert_eq!(t.resize(&room), Resized::Ok);
    assert_eq!(shown_rows(t), (vec![24], 0), "on the row a growth shows");
    let row = t.row(Row::from_index(24)).expect("the grown row");
    let text: String = row
        .iter()
        .take(6)
        .map(|c| {
            c.content
                .as_codepoint()
                .map_or(' ', |cp| char::from_u32(cp.value()).unwrap_or('?'))
        })
        .collect();
    assert_eq!(text, "MARKER", "its text beside it");
}

/// **`?1049 h` on the alternate screen clears a picture kept below it**:
/// "all images in the alternate screen must be cleared, just as all text
/// is cleared" (`[[source: kitty-graphics]]`, "Interaction with other
/// terminal actions"). Entered by `?47`, which clears nothing, the
/// picture pushed below, then `?1049 h`, whose clear takes it.
#[test]
fn a_clearing_entry_takes_a_picture_kept_below_the_alternate_screen() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        push_below(t, b"\x1b[?47h", &mut r);
        assert_eq!(shown_rows(t), (vec![24], 0), "the premise");
        feed(t, b"\x1b[?1049h\x1b[24;1H\n", &mut r);
        assert_eq!(shown_rows(t), (vec![], 0), "cleared");
    });
}

/// **A delete of what is visible leaves a picture below the screen, and a
/// delete by its id takes it**: `d=a` deletes "all placements visible on
/// screen" and `d=i` "all images with the specified id"
/// (`[[source: kitty-graphics]]`, "Deleting images"). Kitty's `d=a` takes
/// one below the screen as its `ED 2` does (`[[source: kitty-graphics-c]]`
/// `clear_filter_func_noncell()`), its practice and not followed: it is not
/// visible. On either screen.
#[test]
fn a_delete_of_what_is_visible_leaves_a_picture_below_the_screen() {
    for enter in [&b""[..], b"\x1b[?1049h"] {
        with_history(4096, |t| {
            let mut r = Vec::new();
            push_below(t, enter, &mut r);
            feed(t, &apc("a=d,d=a,q=2", b""), &mut r);
            assert_eq!(
                shown_rows(t),
                (vec![24], 0),
                "{:?}: not visible, kept",
                String::from_utf8_lossy(enter)
            );
            feed(t, &apc("a=d,d=i,i=1,q=2", b""), &mut r);
            assert_eq!(
                shown_rows(t),
                (vec![], 0),
                "{:?}: by its id, gone",
                String::from_utf8_lossy(enter)
            );
        });
    }
}

/// **A put of its image and placement ids replaces a picture below the
/// screen**: "If you send two placements with the same image id and
/// placement id the second one will replace the first"
/// (`[[source: kitty-graphics]]`), so the picture is drawn where the
/// cursor is and is not kept below as a second one. On either screen.
#[test]
fn a_put_of_its_pair_replaces_a_picture_below_the_screen() {
    for enter in [&b""[..], b"\x1b[?1049h"] {
        with_history(4096, |t| {
            let mut r = Vec::new();
            push_below(t, enter, &mut r);
            feed(t, b"\x1b[5;1H", &mut r);
            feed(t, &apc("a=p,i=1,p=7,q=2,C=1", b""), &mut r);
            assert_eq!(
                shown_rows(t),
                (vec![4], 0),
                "{:?}: one placement, where the cursor is",
                String::from_utf8_lossy(enter)
            );
        });
    }
}

/// The live screen's placements as rows from its top, sorted, and how
/// many placements the store holds on both screens: a placement on the
/// screen not shown is held and not drawn.
fn live_rows(t: &Term<'_>) -> (Vec<i64>, usize) {
    let o = t.image_origin(Rows::ZERO);
    let top = i64::try_from(o.top.value()).unwrap();
    let mut rows: Vec<i64> = t
        .images()
        .placements()
        .filter(|(_, p)| p.screen == o.screen)
        .map(|(_, p)| i64::try_from(p.line.value()).unwrap() - top)
        .collect();
    rows.sort_unstable();
    (rows, t.images().placements().count())
}

/// **A Sixel a reverse scroll pushes below the alternate screen goes**,
/// counted: the Sixel texts describe no row below a screen, and xterm's
/// reverse scroll moves no graphic at all (`[[source: xterm-scroll]]`'s
/// file, `RevScroll`). The kitty picture's rule is kitty's alone.
#[test]
fn a_sixel_pushed_below_the_alternate_screen_goes() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[?1049h\x1b[23;1H", &mut r);
        feed(t, SIXEL, &mut r);
        assert_eq!(shown_rows(t), (vec![22], 0), "the premise");
        feed(t, b"\x1b[1;1H\x1bM", &mut r);
        assert_eq!(shown_rows(t), (vec![23], 0), "on the bottom row, kept");
        feed(t, b"\x1bM", &mut r);
        assert_eq!(shown_rows(t), (vec![], 1), "below it, gone and counted");
    });
}

/// **A change of the screen's geometry drops a picture below the
/// alternate screen**, counted, and so does a change of the cell's
/// height: its row went when a reverse scroll pushed it off, so a change
/// of height would show it over a blank row, apart from the row it was
/// placed on, and a change of width drops it too, where kitty's
/// `grman_resize` and `grman_rescale` keep it
/// (`[[source: kitty-graphics-c]]`). One on the bottom row that a
/// shorter screen cuts goes too, as the screen keeps no row below it.
#[test]
fn a_resize_drops_a_picture_below_the_alternate_screen() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        push_below(t, b"\x1b[?1049h", &mut r);
        t.set_cell_size(ImagePx::of(10), ImagePx::of(24));
        assert_eq!(shown_rows(t), (vec![], 1), "a taller cell: dropped");
    });
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[?1049h\x1b[24;1H", &mut r);
        feed(
            t,
            &apc("a=T,f=24,s=10,v=20,i=1,q=2,C=1", &b64(&[7u8; 600])),
            &mut r,
        );
        assert_eq!(shown_rows(t), (vec![23], 0), "the premise");
        let ws = WinSize {
            ws_row: 22,
            ws_col: 80,
        };
        assert_eq!(t.resize(&ws), Resized::Ok);
        assert_eq!(shown_rows(t), (vec![], 1), "cut by a shorter screen");
    });
    for (cols, rows) in [(80, 26), (60, 24)] {
        // Room for two more rows than the screen starts with.
        let room = WinSize {
            ws_row: 26,
            ws_col: 80,
        };
        let mut owned = Owned::with_history(&room, &CacheGeometry::UNKNOWN, Depth::of(6))
            .with_images(ImageBudget::of(4096));
        let mut t = Term::new(owned.storage(), &WS);
        t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
        let t = &mut t;
        let mut r = Vec::new();
        push_below(t, b"\x1b[?1049h", &mut r);
        assert_eq!(shown_rows(t), (vec![24], 0), "the premise");
        let ws = WinSize {
            ws_row: rows,
            ws_col: cols,
        };
        assert_eq!(t.resize(&ws), Resized::Ok);
        assert_eq!(shown_rows(t), (vec![], 1), "{cols}x{rows}: dropped");
    }
}

/// **A resize to the size the screen has keeps a picture below the
/// alternate screen**, as setting the cell's size it has does: nothing
/// is cut or moved, so a line feed at the bottom brings it back as
/// before. An embedder may call either on every configure of its window,
/// a drag that crosses no cell's edge included.
#[test]
fn a_resize_to_the_same_size_keeps_a_picture_below_the_alternate_screen() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        push_below(t, b"\x1b[?1049h", &mut r);
        assert_eq!(shown_rows(t), (vec![24], 0), "the premise");
        assert_eq!(t.resize(&WS), Resized::Ok);
        assert_eq!(shown_rows(t), (vec![24], 0), "the same size: kept");
        t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
        assert_eq!(shown_rows(t), (vec![24], 0), "the same cell: kept");
        feed(t, b"\x1b[24;1H\n", &mut r);
        assert_eq!(shown_rows(t), (vec![23], 0), "back on the bottom row");
    });
}

/// **Leaving the alternate screen takes its kitty pictures, one kept below
/// it included**, by `?1049 l`, `?47 l` and `?1047 l` alike, each
/// counted, and nothing comes back on a return and a line feed: the
/// editor's images go with the editor's screen. Kitty keeps them with
/// the screen's text, so its `?47 h` shows them again
/// (`screen_toggle_screen_buffer()` in `[[source: kitty-screen-c]]`), the
/// open in docs/vt-coverage.md's `APC G a=p` row.
#[test]
fn leaving_the_alternate_screen_takes_its_kitty_pictures_one_kept_below_too() {
    let trips: [(&[u8], &[u8]); 3] = [
        (b"\x1b[?1049l", b"\x1b[?1049h"),
        (b"\x1b[?47l", b"\x1b[?47h"),
        (b"\x1b[?1047l", b"\x1b[?1047h"),
    ];
    for (leave, back) in trips {
        with_history(4096, |t| {
            let mut r = Vec::new();
            push_below(t, b"\x1b[?47h", &mut r);
            feed(t, b"\x1b[5;1H", &mut r);
            feed(t, &apc("a=p,i=1,p=3,q=2,C=1", b""), &mut r);
            assert_eq!(live_rows(t), (vec![4, 24], 2), "the premise");
            let trip = String::from_utf8_lossy(&[leave, back].concat()).into_owned();
            let expired = t.images().placements_expired;
            feed(t, leave, &mut r);
            assert_eq!(live_rows(t), (vec![], 0), "{trip:?}: gone with the screen");
            let counted = match leave == b"\x1b[?1047l" {
                // `?1047 l` clears first, as `ED 2` does, and the clear's
                // deletes are not expiries.
                true => 0,
                false => 2,
            };
            assert_eq!(
                t.images().placements_expired,
                expired + counted,
                "{trip:?}: counted"
            );
            feed(t, back, &mut r);
            feed(t, b"\x1b[24;1H\n", &mut r);
            assert_eq!(live_rows(t), (vec![], 0), "{trip:?}: nothing back");
        });
    }
}

/// The same on the alternate screen, which keeps no history: kitty's
/// limit there is the screen's top (`[[source: kitty-screen-c]]`
/// `INDEX_GRAPHICS`, `is_main ? -self->historybuf->ynum : 0`), so a
/// picture whose anchor scrolled off the top keeps its rows still on
/// the screen.
#[test]
fn a_tall_picture_on_the_alternate_screen_goes_with_its_last_row() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[?1049h\x1b[2;1H", &mut r);
        feed(
            t,
            &apc("a=T,f=24,s=10,v=40,i=1,r=4,q=2,C=1", &b64(&[7u8; 1200])),
            &mut r,
        );
        let line = kitty_lines(t);
        assert_eq!(line.len(), 1, "the premise: placed");
        feed(t, b"\x1b[24;1H\n\n", &mut r);
        assert_eq!(kitty_lines(t), line, "its anchor off the top, kept");
        feed(t, b"\n\n", &mut r);
        assert_eq!(kitty_lines(t), line, "its last row on the top row, kept");
        feed(t, b"\n", &mut r);
        assert_eq!(kitty_lines(t), Vec::<u64>::new(), "its last row gone");
    });
}

/// **The rows a margin scroll cut are not counted**: a picture two rows
/// tall moved down by `SD` inside margins 5 to 10 shows one row, its last
/// cut at the margin (`[[source: kitty-graphics]]`, "they must be
/// clipped"), so it goes when that one row leaves the history -- kitty's
/// `effective_num_rows`, which its margin filter reduces.
#[test]
fn a_cut_picture_goes_when_its_last_shown_row_leaves_the_history() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[5;10r\x1b[9;1H", &mut r);
        feed(
            t,
            &apc("a=T,f=24,s=10,v=40,i=1,q=2,C=1", &b64(&[7u8; 1200])),
            &mut r,
        );
        feed(t, b"\x1b[T\x1b[r", &mut r);
        assert_eq!(kitty_lines(t), vec![9], "the premise: moved down a row");
        scroll_until(t, 9, &mut r);
        assert_eq!(kitty_lines(t), vec![9], "its one shown row kept");
        scroll_until(t, 10, &mut r);
        assert_eq!(kitty_lines(t), Vec::<u64>::new(), "its cut row not counted");
    });
}

/// A three-row Sixel: four bands of six sixels at the default 2:1
/// aspect are 48 pixels, over three 20-pixel rows; `P2` 1, so no
/// background fill reaches past its data.
const TALL_SIXEL: &[u8] = b"\x1bP0;1q#0;2;100;0;0#0~~-~~-~~-~~\x1b\\";

/// **A tall Sixel goes when its last row leaves the history, not its
/// first**: "The top line of the sixel image may scroll off the screen"
/// (`[[source: vt3xx-sixel]]`, "Sixel Scrolling Enabled"), the rest of
/// it still shown, and xterm moves a graphic up a row a scroll and drops
/// none (`[[source: xterm-graphics-c]]` 1787-1808). Its rows are the
/// `ceil(h / cell_h)` it is drawn over, as text written over it counts
/// them. Three rows on line 2 are kept with their anchor gone, kept with
/// only the last retained, and gone, counted, once that row goes.
#[test]
fn a_tall_sixel_goes_when_its_last_row_leaves_the_history() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[3;1H", &mut r);
        feed(t, TALL_SIXEL, &mut r);
        assert_eq!(visible(t), vec![(2, 48)], "the premise: three rows tall");
        scroll_until(t, 3, &mut r);
        assert_eq!(visible(t).len(), 1, "its anchor gone, two rows kept");
        scroll_until(t, 4, &mut r);
        assert_eq!(visible(t).len(), 1, "its last row kept");
        assert_eq!(t.images().placements_expired, 0);
        scroll_until(t, 5, &mut r);
        assert_eq!(visible(t), Vec::<(u16, u16)>::new(), "its last row gone");
        assert_eq!(t.images().placements_expired, 1, "and counted");
    });
}

/// **The same at the alternate screen's top**, the case the VT3xx text
/// describes: a three-row Sixel on row 2 whose top line scrolled off the
/// screen keeps its rows still on it, and goes, counted, with its last.
#[test]
fn a_tall_sixel_on_the_alternate_screen_goes_with_its_last_row() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[?1049h\x1b[2;1H", &mut r);
        feed(t, TALL_SIXEL, &mut r);
        assert_eq!(visible(t), vec![(2, 48)], "the premise: placed");
        feed(t, b"\x1b[24;1H\n\n", &mut r);
        assert_eq!(visible(t).len(), 1, "its top line off the screen, kept");
        feed(t, b"\n", &mut r);
        assert_eq!(visible(t).len(), 1, "its last row on the top row, kept");
        assert_eq!(t.images().placements_expired, 0);
        feed(t, b"\n", &mut r);
        assert_eq!(visible(t), Vec::<(u16, u16)>::new(), "its last row gone");
        assert_eq!(t.images().placements_expired, 1, "and counted");
    });
}

/// **`ED 3` keeps a Sixel with rows on screen**, as it keeps a kitty
/// picture: xterm's erase of the saved lines zeroes their count and
/// touches no graphic (`[[source: xterm-scroll]]`, `case 3`). A
/// three-row Sixel on the top row, two lines into the history, keeps its
/// row on screen; one wholly in the history goes, counted.
#[test]
fn erasing_the_history_keeps_a_sixel_with_rows_on_screen() {
    for (lfs, kept) in [(2, true), (3, false)] {
        with_history(4096, |t| {
            let mut r = Vec::new();
            feed(t, TALL_SIXEL, &mut r);
            feed(t, b"\x1b[24;1H", &mut r);
            for _ in 0..lfs {
                feed(t, b"\n", &mut r);
            }
            assert_eq!(visible(t).len(), 1, "{lfs}: the premise, in the history");
            feed(t, b"\x1b[3J", &mut r);
            match kept {
                true => assert_eq!(visible(t).len(), 1, "{lfs}: a row on screen, kept"),
                false => {
                    assert_eq!(visible(t), Vec::<(u16, u16)>::new(), "{lfs}: gone");
                    assert_eq!(t.images().placements_expired, 1, "{lfs}: counted");
                }
            }
        });
    }
}

/// **A taller cell counts a Sixel's rows anew**: the three rows of a
/// 48-pixel Sixel at 20 pixels a row are one at 48, so with its anchor
/// gone from the history and two rows kept, a change of cell size takes
/// it at once, counted -- the rows it is drawn over are all gone.
#[test]
fn a_taller_cell_takes_a_sixel_whose_rows_are_then_all_gone() {
    with_history(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[3;1H", &mut r);
        feed(t, TALL_SIXEL, &mut r);
        scroll_until(t, 3, &mut r);
        assert_eq!(visible(t).len(), 1, "the premise: two rows kept");
        t.set_cell_size(ImagePx::of(10), ImagePx::of(48));
        assert_eq!(visible(t), Vec::<(u16, u16)>::new(), "one row, gone");
        assert_eq!(t.images().placements_expired, 1, "and counted");
    });
}

/// A 10 by 40 image placed at `row` (one-based) with `keys` under a
/// region anchored at the top, lines 1 to 20 of 24, whose scroll moves the
/// window; then one LF on the region's bottom row. Returns the
/// placements' lines and the window's top. A line is absolute: one that
/// stays on its screen row reads one more once the window has moved.
fn under_top_region(row: u32, keys: &str) -> (Vec<u64>, u64) {
    with(4096, |t| {
        let mut r = Vec::new();
        feed(t, b"\x1b[1;20r", &mut r);
        feed(t, format!("\x1b[{row};1H").as_bytes(), &mut r);
        let ctrl = format!("a=T,f=24,s=10,v=40,i=1,q=2,C=1{keys}");
        feed(t, &apc(&ctrl, &b64(&[7u8; 1200])), &mut r);
        feed(t, b"\x1b[20;1H\n", &mut r);
        (
            kitty_lines(t),
            t.image_origin(baud_domain::Rows::ZERO).top.value(),
        )
    })
}

/// A region anchored at the top scrolls by moving the window, and puts
/// the rows under it back: an image under it goes back with them however
/// far past the screen it reaches, and one across its bottom margin, not
/// wholly inside, stays where it is with the text under it.
#[test]
fn a_top_anchored_region_leaves_the_images_it_does_not_scroll() {
    // Rows 23 to 27, under the region and past the screen's bottom.
    let (lines, top) = under_top_region(23, ",r=5");
    assert_eq!(top, 1, "the premise: the window moved");
    assert_eq!(lines, vec![23], "under the region, put back");
    // Rows 20 and 21, across the region's bottom margin.
    let (lines, top) = under_top_region(20, "");
    assert_eq!(top, 1, "the premise: the window moved");
    assert_eq!(lines, vec![20], "across the margin, left");
    // The control: rows 18 and 19, inside, scroll with the region.
    // Its line is unchanged and the window moved past it: a row up.
    assert_eq!(under_top_region(18, "").0, vec![17], "inside, scrolled");
}

/// **A body reaches the stream a run at a time, not a byte at a time**:
/// everything after the `G` up to its `ESC \` is printable ASCII, so one
/// read hands it over in one call. Fed a byte at a time it was one call a
/// byte, 21,875 here, about 4 ns each: a 1920x1080 `f=32` took 46-47 ms,
/// all of it feeding.
#[test]
fn a_body_reaches_the_stream_in_one_call_a_run() {
    let data = bytes(64 * 64 * 4, 31);
    let s = apc("a=T,f=32,s=64,v=64,i=3,q=2", &b64(&data));
    with(1 << 13, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        assert_eq!(placed_pixels(t), rgba_pixels(&data), "the pixels sent");
        assert_eq!(t.kitty_feed().runs, 1, "one run, the body");
    });
    let p = png(16, 16, png::ColorType::Rgba, &bytes(16 * 16 * 4, 32), 100);
    let s = chunked("a=T,f=100,i=4,q=2", "a=T,f=100,q=2", &b64(&p), 512);
    let chunks = s.windows(3).filter(|w| *w == b"\x1b_G").count();
    with(1 << 13, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        settle(t, &mut r);
        assert_eq!(t.kitty().images, 1);
        assert_eq!(t.kitty_feed().runs, chunks, "one run a chunk");
    });
}

/// A raw transmission with `extra` spliced into its payload at `at`.
fn spliced(at: usize, extra: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let data = bytes(3 * 2 * 4, 40);
    let mut enc = b64(&data);
    let tail = enc.split_off(at);
    enc.extend_from_slice(extra);
    enc.extend_from_slice(&tail);
    (apc("a=T,f=32,s=3,v=2,i=7", &enc), data)
}

/// **A byte inside the payload that is no base64 ends the data there, the
/// bulk path as the byte path**: kitty keeps a C0, `DEL`, `=`, `*` and `;`
/// in the command (`find_st_terminator`, `[[source: kitty-vt-parser-c]]`)
/// and its decode stops at the first of them, keeping the octets before it
/// (`KittyBase64`), so the 24 bytes come up short by what follows and the
/// reply is ENODATA with what there was: `at` characters' whole octets. A
/// `BEL` ends the command there as well, and what followed it is text. At
/// every read size and every two-read split. These were Williams' rule
/// until kitty's handling of its own protocol: a C0 ignored and the run
/// going on past it, and the rest refused as base64.
#[test]
fn what_interrupts_a_payload_run_is_what_it_was_byte_by_byte() {
    for at in [1, 2, 3, 4, 13, 30] {
        let short = format!(
            "\x1b_Gi=7;ENODATA:Insufficient image data: {} < 24\x1b\\",
            at * 3 / 4
        );
        for stop in [
            &b"\n"[..],
            b"\0",
            b"\x07",
            b"\x08\r",
            b"\x1f",
            b"\x7f",
            b"=",
            b"*",
            b";",
        ] {
            let (s, _) = spliced(at, stop);
            invariant(64, &s, Splits::Every);
            with(64, |t| {
                let mut r = Vec::new();
                feed(t, &s, &mut r);
                assert_eq!(t.images().placements().count(), 0, "{at} {stop:?}");
                assert_eq!(String::from_utf8_lossy(&r), short, "{at} {stop:?}");
            });
        }
    }
}

/// **`CAN`, `SUB` and an `ESC` that is not `ST` are payload of a kitty
/// command, mid-run as anywhere**: kitty ends the command only at `BEL` or
/// `ESC \` (`find_st_terminator`, `[[source: kitty-vt-parser-c]]`), so each
/// stays in the payload, the data ends at it with nine characters' six
/// octets (`KittyBase64`), and the command it would have cancelled is
/// refused short at its own `ST`; the next command is its own. Nothing is
/// cancelled. At every split. Under Williams' rule each cancelled the chunk;
/// this was the test a cancel inside a payload run ends it where it stands.
#[test]
fn a_cancel_inside_a_payload_run_is_payload_and_ends_the_data() {
    for cancel in [&b"\x18"[..], b"\x1a", b"\x1bX", b"\x1b7", b"\x1b_G"] {
        let (mut s, _) = spliced(9, cancel);
        s.extend(apc("a=T,f=24,s=1,v=1,i=2", &b64(&[1, 2, 3])));
        invariant(64, &s, Splits::Every);
        with(64, |t| {
            let mut r = Vec::new();
            feed(t, &s, &mut r);
            assert_eq!(t.kitty().cancelled, 0, "{cancel:?}");
            assert_eq!(
                String::from_utf8_lossy(&r),
                "\x1b_Gi=7;ENODATA:Insufficient image data: 6 < 24\x1b\\\x1b_Gi=2;OK\x1b\\",
                "{cancel:?}"
            );
        });
    }
}

/// **Under ISO 8859-1 a GR byte of the body is its 7-bit byte, and `9C` is
/// payload in both encodings**: `[[source: dec-std-070]]` 3.5.4.5 for the
/// first; for the second kitty ends a command only at `BEL` or `ESC \`
/// (`find_st_terminator`, `[[source: kitty-vt-parser-c]]`), so `9C` is no
/// `ST` inside one, and no base64, so the data ends at it: ten characters'
/// seven octets of the 24. The run the bulk path takes is GL only, so these
/// bytes take the byte path, at every split. Under ISO 8859-1 `9C` was the
/// 8-bit `ST` here until kitty's handling of its own protocol.
#[test]
fn gr_payload_and_the_one_byte_st_keep_their_meaning_beside_a_run() {
    let data = bytes(3 * 2 * 4, 41);
    let enc = b64(&data);
    let mut gr = b"\x1b%@\x1b_Ga=T,f=32,s=3,v=2,i=8;".to_vec();
    gr.extend(enc.iter().enumerate().map(|(k, &b)| match k % 3 {
        0 => b | 0x80,
        _ => b,
    }));
    gr.extend_from_slice(b"\x1b\\");
    invariant(64, &gr, Splits::Every);
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &gr, &mut r);
        assert_eq!(placed_pixels(t), rgba_pixels(&data));
        assert_eq!(r, b"\x1b_Gi=8;OK\x1b\\");
    });
    for head in [&b""[..], b"\x1b%@"] {
        let mut c1 = head.to_vec();
        c1.extend_from_slice(b"\x1b_Ga=T,f=32,s=3,v=2,i=8;");
        c1.extend_from_slice(&enc[..10]);
        c1.push(0x9c);
        c1.extend_from_slice(&enc[10..]);
        c1.extend_from_slice(b"\x1b\\");
        invariant(64, &c1, Splits::Every);
        with(64, |t| {
            let mut r = Vec::new();
            feed(t, &c1, &mut r);
            assert_eq!(t.images().placements().count(), 0, "{head:?}");
            assert_eq!(
                String::from_utf8_lossy(&r),
                "\x1b_Gi=8;ENODATA:Insufficient image data: 7 < 24\x1b\\",
                "{head:?}"
            );
        });
    }
}

/// **A refusal mid-run is the one the byte path made**: a PNG whose
/// header is refused at its 24th octet, too wide for the arena, and whose
/// payload then carries a byte that is no base64, answers for the header
/// and not for the base64: past a refusal the byte path decoded nothing.
#[test]
fn a_refusal_inside_a_run_is_the_first_one_byte_by_byte() {
    let p = png(4000, 1, png::ColorType::Rgb, &vec![9u8; 4000 * 3], 1 << 16);
    let mut enc = b64_unpadded(&p);
    enc.truncate(64);
    enc.extend_from_slice(b"*AAAA");
    let s = apc("a=T,f=100,i=9", &enc);
    invariant(64, &s, Splits::Every);
    let reply = with(64, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        settle(t, &mut r);
        r
    });
    assert_eq!(
        String::from_utf8_lossy(&reply),
        "\x1b_Gi=9;EINVAL:Image too large for the image arena\x1b\\"
    );
}

proptest::proptest! {
    /// **Any kitty stream, cut anywhere, is the stream fed whole**: raw,
    /// zlib and PNG transmissions in chunks of any length, a PNG whose
    /// header is refused among them, with a
    /// C0, a byte that is no base64, a `CAN`, a `SUB` or a bare `ESC`
    /// spliced in at any byte, then fed in reads of any size, a byte at
    /// a time, and in two reads cut anywhere. Run at `PROPTEST_CASES=20000`.
    #[test]
    fn any_kitty_stream_cut_anywhere_is_the_stream_fed_whole(
        kinds in proptest::collection::vec((0u8..4, 1usize..40, 0u32..1000), 1..3),
        splices in proptest::collection::vec((0usize..10_000, 0usize..10), 0..3),
        read in 1usize..300,
        cut in 0usize..10_000,
    ) {
        let mut s = Vec::new();
        for (k, (kind, n, seed)) in kinds.into_iter().enumerate() {
            let data = bytes(2 * 3 * 4, seed);
            let (keys, enc) = match kind {
                0 => ("a=T,f=32,s=2,v=3", b64(&data)),
                1 => ("a=T,f=32,s=2,v=3,o=z", b64_unpadded(&zlib_mixed(&data))),
                2 => ("a=T,f=100", b64(&png(2, 3, png::ColorType::Rgba, &data, 9))),
                // Wider than the arena: refused at its 24th octet, mid-run.
                _ => ("a=T,f=100", b64(&png(4000, 1, png::ColorType::Rgb, &[7; 12_000], 9))),
            };
            // An id, so every outcome is answered and its error compared.
            s.extend(chunked(&format!("{keys},i={}", k + 1), "", &enc, n));
        }
        const SPLICE: [&[u8]; 10] = [
            b"\n", b"\0", b"\x7f", b"=", b"*", b"\x18", b"\x1a", b"\x1bX", b"\x1b\\", b"\x07",
        ];
        for (at, k) in splices {
            let at = at % (s.len() + 1);
            let tail = s.split_off(at);
            s.extend_from_slice(SPLICE[k]);
            s.extend_from_slice(&tail);
        }
        let whole = with(256, |t| {
            let mut r = Vec::new();
            feed(t, &s, &mut r);
            settle(t, &mut r);
            state(t, &r)
        });
        let reads = with(256, |t| {
            let mut r = Vec::new();
            for piece in s.chunks(read) {
                feed(t, piece, &mut r);
            }
            settle(t, &mut r);
            state(t, &r)
        });
        proptest::prop_assert_eq!(&reads, &whole, "reads of {}", read);
        // A byte a read: every run one byte long, the byte path's order.
        let bytewise = with(256, |t| {
            let mut r = Vec::new();
            for piece in s.chunks(1) {
                feed(t, piece, &mut r);
            }
            settle(t, &mut r);
            state(t, &r)
        });
        proptest::prop_assert_eq!(&bytewise, &whole, "a byte a read");
        let cut = cut % (s.len() + 1);
        let two = with(256, |t| {
            let mut r = Vec::new();
            feed(t, &s[..cut], &mut r);
            feed(t, &s[cut..], &mut r);
            settle(t, &mut r);
            state(t, &r)
        });
        proptest::prop_assert_eq!(&two, &whole, "cut at {}", cut);
    }
}

// ---- the file mediums, through the embedder's reader ----------------------

use baud_domain::{KittyFileFailed, KittyFileRequest, KittyFiles, KittyFlow, KittyPastLimit};

/// One request as the reader saw it: its keys, and its path kept.
#[derive(Clone, Debug, PartialEq)]
struct Asked {
    req: KittyFileRequest<'static>,
    path: Vec<u8>,
}

/// What a reader saw, kept where the test can read it after lending a
/// copy of the reader away.
#[derive(Default)]
struct Log {
    asked: Vec<Asked>,
    /// Each piece handed over, by its length.
    pieces: Vec<usize>,
}

/// **A reader over named byte strings**, standing in for a filesystem: it
/// records every request, decides the length by the request's own rule,
/// and hands the bytes over `chunk` at a time.
#[derive(Clone)]
struct Mem {
    named: Vec<(Vec<u8>, Vec<u8>)>,
    chunk: core::num::NonZeroUsize,
    log: std::rc::Rc<std::cell::RefCell<Log>>,
}

impl Mem {
    fn new(named: &[(&[u8], &[u8])], chunk: usize) -> Mem {
        Mem {
            named: named
                .iter()
                .map(|(p, d)| (p.to_vec(), d.to_vec()))
                .collect(),
            chunk: core::num::NonZeroUsize::new(chunk).unwrap(),
            log: Default::default(),
        }
    }

    /// The requests seen.
    fn asked(&self) -> Vec<Asked> {
        self.log.borrow().asked.clone()
    }

    /// The bytes handed over.
    fn handed(&self) -> usize {
        self.log.borrow().pieces.iter().sum()
    }
}

impl KittyFiles for Mem {
    fn read(
        &mut self,
        req: &KittyFileRequest<'_>,
        sink: &mut dyn FnMut(&[u8]) -> KittyFlow,
    ) -> Result<(), KittyFileFailed> {
        self.log.borrow_mut().asked.push(Asked {
            req: KittyFileRequest { path: b"", ..*req },
            path: req.path.to_vec(),
        });
        let data = match self.named.iter().find(|(p, _)| p == req.path) {
            Some((_, d)) => d.clone(),
            None => return Err(KittyFileFailed),
        };
        let len = u64::try_from(data.len()).unwrap();
        let n = req.read_len(req.available(len))?;
        let from = usize::try_from(req.offset).unwrap().min(data.len());
        for c in data[from..from + n].chunks(self.chunk.get()) {
            self.log.borrow_mut().pieces.push(c.len());
            if sink(c) == KittyFlow::Stop {
                break;
            }
        }
        Ok(())
    }
}

/// A terminal with `pixels` of arena, a 10x20 cell, and a copy of `files`
/// lent, leaked as an embedder leaks it.
fn with_files<R>(pixels: usize, files: &Mem, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(pixels));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    t.set_kitty_files(Box::leak(Box::new(files.clone())));
    f(&mut t)
}

/// How many placements a terminal at [`WS`] holds: what a test fills to
/// refuse the next one.
fn placement_slots() -> usize {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(64));
    let t = Term::new(owned.storage(), &WS);
    t.images().placement_slots()
}

/// The single error, for image 1.
const FILE_ERR: &[u8] = b"\x1b_Gi=1;EBADF:Failed to read image file\x1b\\";

/// **Each file medium decodes what `t=d` decodes**, in every format: the
/// state after the bytes come from the reader is the state after they come
/// in the payload, and the reader was asked for the payload's path with
/// the medium named.
#[test]
fn a_file_medium_reads_through_the_embedder_and_decodes_as_direct() {
    let rgb = bytes(5 * 3 * 3, 81);
    let rgba = bytes(5 * 3 * 4, 82);
    let p = png(6, 4, png::ColorType::Rgba, &bytes(6 * 4 * 4, 83), 37);
    let zrgb = zlib_mixed(&rgb);
    let zpng = zlib_mixed(&p);
    let cases: [(String, &[u8]); 5] = [
        ("f=24,s=5,v=3".to_string(), &rgb),
        ("f=32,s=5,v=3".to_string(), &rgba),
        ("f=100".to_string(), &p),
        ("f=24,s=5,v=3,o=z".to_string(), &zrgb),
        (format!("f=100,o=z,S={}", p.len()), &zpng),
    ];
    for (keys, data) in &cases {
        let direct = with(256, |t| {
            let mut r = Vec::new();
            feed(t, &apc(&format!("a=T,i=1,{keys}"), &b64(data)), &mut r);
            settle(t, &mut r);
            state(t, &r)
        });
        assert!(direct.contains("i=1;OK"), "{keys}: {direct}");
        for (medium, name) in [
            ('f', &b"/pics/a.raw"[..]),
            ('t', b"/tmp/tty-graphics-protocol-a"),
            ('s', b"/shm-a"),
        ] {
            for chunk in [1, 7, 4096] {
                let m = Mem::new(&[(name, data)], chunk);
                let got = with_files(256, &m, |t| {
                    let mut r = Vec::new();
                    let cmd = apc(&format!("a=T,i=1,t={medium},{keys}"), &b64(name));
                    feed(t, &cmd, &mut r);
                    settle(t, &mut r);
                    state(t, &r)
                });
                assert_eq!(got, direct, "{keys} t={medium} chunk {chunk}");
                assert_eq!(m.asked().len(), 1, "{keys} t={medium}");
                assert_eq!(m.asked()[0].path, name, "the payload is the path");
                let want = match medium {
                    'f' => baud_domain::KittyMedium::File,
                    't' => baud_domain::KittyMedium::TempFile,
                    _ => baud_domain::KittyMedium::SharedMemory,
                };
                assert_eq!(m.asked()[0].req.medium, want);
            }
        }
    }
}

/// **A PNG query and a PNG frame through a file medium decode as `t=d`'s
/// do**: after a root image, `a=q` and `a=f` with `f=100` through `t=f`,
/// `t=t` and `t=s` leave the replies, the placements, the frames and the
/// new frame's pixels that the PNG sent in the payload leaves.
#[test]
fn a_png_query_and_a_png_frame_through_a_file_decode_as_direct() {
    let p = png(6, 4, png::ColorType::Rgba, &bytes(6 * 4 * 4, 84), 37);
    let root = apc("a=t,i=1,f=24,s=6,v=4,q=2", &b64(&bytes(6 * 4 * 3, 85)));
    let run = |medium: Option<char>, keys: &str| {
        let m = Mem::new(&[(b"/pics/p.png", &p)], 7);
        with_files(256, &m, |t| {
            let mut r = Vec::new();
            feed(t, &root, &mut r);
            let cmd = match medium {
                None => apc(keys, &b64(&p)),
                Some(c) => apc(&format!("{keys},t={c}"), &b64(b"/pics/p.png")),
            };
            feed(t, &cmd, &mut r);
            settle(t, &mut r);
            let imgs = t.images();
            let img = imgs.kitty_image(baud_domain::KittyId::of(1)).unwrap();
            let frames = imgs.kitty_frames(img).map(|f| f.0);
            let px: Option<Vec<u32>> = imgs
                .frame_pixels(img, baud_domain::FrameNo::of(2))
                .map(|v| v.iter().map(|p| p.bits()).collect());
            format!("{}frames {frames:?} {px:?}", state(t, &r))
        })
    };
    for (keys, ok) in [("a=q,i=1,f=100", "i=1;OK"), ("a=f,i=1,f=100", "i=1,r=2;OK")] {
        let direct = run(None, keys);
        assert!(direct.contains(ok), "{keys}: {direct}");
        for c in ['f', 't', 's'] {
            assert_eq!(run(Some(c), keys), direct, "{keys} t={c}");
        }
    }
}

/// **An `a=f` frame reads through a file medium** as an image's data
/// does: the reader asked once for the payload's path, the frame answered
/// with its number, `i=1,r=2;OK`, the image two frames after, and the new
/// frame's pixels the file's, however the reader hands them over.
#[test]
fn an_animation_frame_reads_through_a_file_medium() {
    let rgb = bytes(2 * 2 * 3, 95);
    for chunk in [1, 5, 4096] {
        let m = Mem::new(&[(b"/frame", &rgb)], chunk);
        with_files(256, &m, |t| {
            let mut r = Vec::new();
            feed(
                t,
                &apc("a=t,i=1,f=24,s=2,v=2,q=2", &b64(&[0u8; 12])),
                &mut r,
            );
            settle(t, &mut r);
            assert!(r.is_empty(), "the premise: {}", String::from_utf8_lossy(&r));
            feed(t, &apc("a=f,i=1,t=f,f=24,s=2,v=2", &b64(b"/frame")), &mut r);
            settle(t, &mut r);
            assert_eq!(
                String::from_utf8_lossy(&r),
                "\x1b_Gi=1,r=2;OK\x1b\\",
                "chunk {chunk}"
            );
            let imgs = t.images();
            let img = imgs.kitty_image(baud_domain::KittyId::of(1)).unwrap();
            assert_eq!(
                imgs.kitty_frames(img).map(|f| f.0),
                Some(2),
                "chunk {chunk}"
            );
            let got: Vec<u8> = imgs
                .frame_pixels(img, baud_domain::FrameNo::of(2))
                .expect("frame 2")
                .iter()
                .flat_map(|p| {
                    let (r, g, b) = p.rgb();
                    [r.get(), g.get(), b.get()]
                })
                .collect();
            assert_eq!(got, rgb, "chunk {chunk}: the file's pixels");
        });
        assert_eq!(m.asked().len(), 1, "chunk {chunk}");
        assert_eq!(m.asked()[0].path, b"/frame");
        assert_eq!(m.asked()[0].req.medium, baud_domain::KittyMedium::File);
    }
}

/// **`O` and `S` choose the bytes**: "read 80 bytes starting from the
/// offset 10" ("Local client"), and an object "smaller than the client
/// claimed" is the single error.
#[test]
fn o_and_s_choose_the_bytes_read_and_a_short_object_is_the_one_error() {
    let rgb = bytes(4 * 2 * 3, 91);
    let mut padded = vec![0xEEu8; 10];
    padded.extend_from_slice(&rgb);
    padded.extend_from_slice(&[0xDD; 7]);
    let name: &[u8] = b"/p/padded";
    let m = Mem::new(&[(name, &padded)], 5);
    with_files(256, &m, |t| {
        let mut r = Vec::new();
        let keys = format!("a=T,i=1,t=f,f=24,s=4,v=2,O=10,S={}", rgb.len());
        feed(t, &apc(&keys, &b64(name)), &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
        assert_eq!(placed_pixels(t), rgb_pixels(&rgb));
    });
    assert_eq!((m.asked()[0].req.offset, m.asked()[0].req.size), (10, 24));
    // S past the object, O past its end, and S short of the pixels: each
    // the one error, and nothing stored or placed.
    for keys in [
        "a=T,i=1,t=f,f=24,s=4,v=2,O=10,S=999",
        "a=T,i=1,t=f,f=24,s=4,v=2,O=999",
        "a=T,i=1,t=f,f=24,s=4,v=2,O=10,S=23",
    ] {
        let m = Mem::new(&[(name, &padded)], 5);
        with_files(256, &m, |t| {
            let mut r = Vec::new();
            feed(t, &apc(keys, &b64(name)), &mut r);
            assert_eq!(r, FILE_ERR, "{keys}: {:?}", String::from_utf8_lossy(&r));
            assert_eq!(t.images().placements().count(), 0, "{keys}");
            assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO), "{keys}");
        });
    }
}

/// **Every failure is one identical reply**: a name the reader does not
/// have, raw data short of `s * v` with no `S` -- which `t=d` answers
/// ENODATA -- and a PNG past the scratch are the same bytes, so the reply
/// tells a remote client nothing about the object.
#[test]
fn every_file_failure_answers_one_identical_reply() {
    let short = bytes(4 * 2 * 3 - 1, 92);
    let big = vec![7u8; 64 * 8 + 1];
    let m = Mem::new(&[(b"/short", &short), (b"/big.png", &big)], 64);
    with_files(64, &m, |t| {
        for (keys, name) in [
            ("a=T,i=1,t=f,f=24,s=4,v=2", &b"/missing"[..]),
            ("a=T,i=1,t=t,f=24,s=4,v=2", b"/short"),
            ("a=T,i=1,t=s,f=100", b"/big.png"),
            ("a=q,i=1,t=f,f=100", b"/big.png"),
        ] {
            let mut r = Vec::new();
            feed(t, &apc(keys, &b64(name)), &mut r);
            settle(t, &mut r);
            assert_eq!(r, FILE_ERR, "{keys}: {:?}", String::from_utf8_lossy(&r));
        }
        assert_eq!(t.images().placements().count(), 0);
        assert_eq!(t.kitty().images, 0);
    });
    // The PNG past the scratch was refused before a byte was handed over.
    assert_eq!(
        m.handed(),
        short.len(),
        "only the short raw object was read"
    );
}

/// **The bound is decided before reading**: raw pixels are read to `s *
/// v` and no further, a PNG is bounded by the scratch and refused past it,
/// and zlib data by the zlib bound of what it inflates to.
#[test]
fn the_bound_is_decided_before_reading() {
    let rgb = bytes(4 * 2 * 3, 93);
    let mut long = rgb.clone();
    long.extend_from_slice(&[1u8; 100]);
    let m = Mem::new(&[(b"/long", &long)], 3);
    with_files(64, &m, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=T,i=1,t=f,f=24,s=4,v=2", &b64(b"/long")), &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
        assert_eq!(placed_pixels(t), rgb_pixels(&rgb));
        assert_eq!(
            t.kitty().excess,
            0,
            "the bytes past the pixels were not read"
        );
    });
    assert_eq!(
        (m.asked()[0].req.limit, m.asked()[0].req.past),
        (24, KittyPastLimit::Stop)
    );
    assert_eq!(m.handed(), 24);
    let p = png(2, 2, png::ColorType::Rgba, &bytes(16, 94), 8);
    let m = Mem::new(&[(b"/p", &p)], 64);
    with_files(64, &m, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=t,i=1,t=f,f=100", &b64(b"/p")), &mut r);
        settle(t, &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
    });
    assert_eq!(
        (m.asked()[0].req.limit, m.asked()[0].req.past),
        (64 * 8, KittyPastLimit::Refuse)
    );
    let z = zlib_mixed(&rgb);
    let m = Mem::new(&[(b"/z", &z)], 64);
    with_files(64, &m, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=t,i=1,t=f,f=24,s=4,v=2,o=z", &b64(b"/z")), &mut r);
        settle(t, &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
    });
    assert_eq!(
        (m.asked()[0].req.limit, m.asked()[0].req.past),
        (24 + 3 + 64, KittyPastLimit::Refuse)
    );
    // A PNG through zlib keeps `S` as the PNG's size and reads to the end.
    let zp = zlib_mixed(&p);
    let m = Mem::new(&[(b"/zp", &zp)], 64);
    with_files(64, &m, |t| {
        let mut r = Vec::new();
        let keys = format!("a=t,i=1,t=f,f=100,o=z,S={}", p.len());
        feed(t, &apc(&keys, &b64(b"/zp")), &mut r);
        settle(t, &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
    });
    assert_eq!(m.asked()[0].req.size, 0, "the whole object");
    assert_eq!(m.asked()[0].req.limit, p.len() + p.len() / 8 + 64);
}

/// **kitten's detection, served**: each medium's query is answered `OK`
/// before the Primary DA, and stores nothing.
#[test]
fn a_served_query_per_medium_is_answered_before_the_primary_da() {
    let px: &[u8] = &[1, 2, 3];
    let m = Mem::new(
        &[(b"/tmp/tty-graphics-protocol-x", px), (b"/shm", px)],
        4096,
    );
    let mut s = apc("i=31,s=1,v=1,a=q,t=d,f=24", b"AAAA");
    s.extend(apc(
        "i=32,s=1,v=1,a=q,t=t,f=24",
        &b64(b"/tmp/tty-graphics-protocol-x"),
    ));
    s.extend(apc("i=33,s=1,v=1,a=q,t=s,f=24", &b64(b"/shm")));
    s.extend(apc("i=34,s=1,v=1,a=q,t=f,f=24", &b64(b"/remote-only")));
    s.extend_from_slice(b"\x1b[c");
    let (r, imgs) = with_files(64, &m, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        (r, t.kitty().images)
    });
    let r = String::from_utf8_lossy(&r).into_owned();
    let expect = "\x1b_Gi=31;OK\x1b\\\x1b_Gi=32;OK\x1b\\\x1b_Gi=33;OK\x1b\\\
        \x1b_Gi=34;EBADF:Failed to read image file\x1b\\";
    assert!(r.starts_with(expect), "the answers first, in order: {r:?}");
    assert!(
        r[expect.len()..].starts_with("\x1b[?"),
        "then the DA: {r:?}"
    );
    assert_eq!(imgs, 0, "a query stores nothing");
}

/// **A path past 2048 bytes is refused without asking**, as kitty refuses
/// it; one of 2048 is asked for. A file medium is one chunk: `m=1` is read
/// at once and the next command is its own.
#[test]
fn a_long_path_is_refused_unasked_and_a_file_medium_is_one_chunk() {
    let px: &[u8] = &[4, 5, 6];
    let at_max = vec![b'a'; 2048];
    let past = vec![b'a'; 2049];
    let m = Mem::new(&[(&at_max, px)], 64);
    with_files(64, &m, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=t,i=1,t=f,f=24,s=1,v=1", &b64(&past)), &mut r);
        assert_eq!(r, b"\x1b_Gi=1;EINVAL:Filename too long\x1b\\");
        r.clear();
        feed(t, &apc("a=t,i=2,t=f,f=24,s=1,v=1", &b64(&at_max)), &mut r);
        assert_eq!(r, b"\x1b_Gi=2;OK\x1b\\");
        r.clear();
        feed(
            t,
            &apc("a=t,i=3,t=f,f=24,s=1,v=1,m=1", &b64(&at_max)),
            &mut r,
        );
        assert_eq!(r, b"\x1b_Gi=3;OK\x1b\\", "answered at its own terminator");
        r.clear();
        feed(t, &apc("a=p,i=3", b""), &mut r);
        assert_eq!(r, b"\x1b_Gi=3;OK\x1b\\", "the next command is not a chunk");
    });
    assert_eq!(m.asked().len(), 2, "the long path was never asked for");
}

/// **A file medium is the same however the reads fall**, and `RIS` keeps
/// the reader: every read size and every two-read split of a stream that
/// sends one by file and places it.
#[test]
fn a_file_medium_is_the_same_however_the_reads_fall() {
    let rgba = bytes(3 * 2 * 4, 95);
    let name: &[u8] = b"/tmp/tty-graphics-protocol-r";
    let mut s = b"\x1bc\x1b[3;4H".to_vec();
    s.extend(apc("a=T,i=1,t=t,f=32,s=3,v=2", &b64(name)));
    s.extend_from_slice(b"after");
    let run = |pieces: &[&[u8]]| {
        let m = Mem::new(&[(name, &rgba)], 5);
        with_files(64, &m, |t| {
            let mut r = Vec::new();
            for p in pieces {
                feed(t, p, &mut r);
            }
            settle(t, &mut r);
            state(t, &r)
        })
    };
    let whole = run(&[&s]);
    assert!(whole.contains("i=1;OK"), "{whole}");
    for n in 1..=s.len() {
        let pieces: Vec<&[u8]> = s.chunks(n).collect();
        assert_eq!(run(&pieces), whole, "read size {n}");
    }
    for cut in 1..s.len() {
        assert_eq!(run(&[&s[..cut], &s[cut..]]), whole, "cut at {cut}");
    }
}

/// **With no reader lent, a file medium touches nothing**: it is answered
/// the one error before anything is reserved, so the image filling the
/// arena is not evicted for it.
#[test]
fn with_no_reader_a_file_medium_reserves_nothing() {
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=t,i=5,f=24,s=8,v=8", &b64(&[9u8; 192])), &mut r);
        r.clear();
        feed(t, &apc("a=t,i=1,t=f,f=24,s=8,v=8", &b64(b"/p")), &mut r);
        assert_eq!(r, FILE_ERR);
        assert_eq!(t.images().images_evicted, 0, "nothing evicted");
        r.clear();
        feed(t, &apc("a=p,i=5", b""), &mut r);
        assert_eq!(r, b"\x1b_Gi=5;OK\x1b\\", "image 5 is still stored");
    });
}

/// **A decode that fails after the read: a bad signature, a broken later
/// `IDAT`, an `IHDR` past the arena, a scanline buffer past the scratch,
/// broken zlib, and a relative placement to a parent image that does not
/// exist after a raw and after a PNG decode** -- each command with the arena it needs and what is fed after
/// it. `t=d` answers each with
/// its own code; this is the control that each reaches that code.
fn after_the_read() -> Vec<AfterRead> {
    let small = png(2, 2, png::ColorType::Rgba, &bytes(16, 3), 8);
    // The last of several `IDAT`s broken: the header and the first chunk
    // parse, so the PNG is reserved, placed and fails in a later step.
    let mut late = png(21, 23, png::ColorType::Rgba, &bytes(21 * 23 * 4, 7), 50);
    let at = late.len() - 12 - 4 - 10;
    late[at] ^= 0xFF;
    let wide = png(9, 8, png::ColorType::Rgb, &bytes(216, 1), 64);
    // 8x8 at sixteen bits a channel: 64 pixels fit the arena, and their
    // 520 bytes of scanlines do not fit the 512-byte scratch.
    let deep = {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, 8, 8);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Sixteen);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&[0u8; 8 * 8 * 8]).unwrap();
        drop(wr);
        out
    };
    [
        (
            "signature",
            Arena::Small,
            "a=T,i=1,f=100".into(),
            b"not a png at all".to_vec(),
            &b""[..],
            "EBADPNG",
        ),
        (
            "late idat",
            Arena::Large,
            "a=T,i=1,f=100".into(),
            late,
            b"",
            "EBADPNG",
        ),
        (
            "ihdr",
            Arena::Small,
            "a=T,i=1,f=100".into(),
            wide,
            b"",
            "EINVAL:Image too large",
        ),
        (
            "scanlines",
            Arena::Small,
            "a=T,i=1,f=100".into(),
            deep,
            b"",
            "EFBIG",
        ),
        (
            "zlib",
            Arena::Small,
            "a=T,i=1,f=24,s=2,v=2,o=z".into(),
            vec![0x78, 0x9c, 0xff, 0xff, 0xff, 0xff],
            b"",
            "EINVAL:Failed to inflate",
        ),
        (
            "relative raw",
            Arena::Small,
            "a=T,i=1,f=24,s=2,v=2,P=5".into(),
            bytes(12, 4),
            b"",
            "ENOPARENT",
        ),
        (
            "relative png",
            Arena::Small,
            "a=T,i=1,f=100,P=5".into(),
            small,
            b"",
            "ENOPARENT",
        ),
    ]
    .into_iter()
    .map(|(what, arena, keys, data, then, code)| AfterRead {
        what,
        arena,
        keys,
        data,
        then,
        code,
    })
    .collect()
}

/// The arena a case of [`after_the_read`] needs: 64 pixels, so a size
/// past it is refused, or 4096, room for a 21x23 picture.
#[derive(Clone, Copy)]
enum Arena {
    Small,
    Large,
}

impl Arena {
    fn pixels(self) -> usize {
        match self {
            Arena::Small => 64,
            Arena::Large => 4096,
        }
    }
}

/// One case of [`after_the_read`]: its name, the arena, the command's
/// keys, the data, what is fed after it, and the code `t=d` answers.
struct AfterRead {
    what: &'static str,
    arena: Arena,
    keys: String,
    data: Vec<u8>,
    then: &'static [u8],
    code: &'static str,
}

/// **Every refusal after a file medium is read is the one reply**: "all
/// failures to read an image file ... must be reported with a single,
/// identical, error response", so that a client cannot "use the responses
/// to these commands to learn anything about files it cannot read itself"
/// ("The transmission medium"). A decode error tells the client the file
/// exists and something of what it holds; through `t=f`, `t=t` and `t=s`
/// each is `EBADF:Failed to read image file`, while `t=d` keeps its code.
#[test]
fn every_refusal_after_the_read_is_the_one_reply() {
    for AfterRead {
        what,
        arena,
        keys,
        data,
        then,
        code,
    } in after_the_read()
    {
        let direct = with(arena.pixels(), |t| {
            let mut r = Vec::new();
            feed(t, &apc(&keys, &b64(&data)), &mut r);
            feed(t, then, &mut r);
            settle(t, &mut r);
            String::from_utf8_lossy(&r).into_owned()
        });
        assert!(
            direct.starts_with(&format!("\x1b_Gi=1;{code}")),
            "{what}: the control reaches {code}: {direct:?}"
        );
        for (medium, name) in [
            ('f', &b"/pics/a"[..]),
            ('t', b"/tmp/tty-graphics-protocol-a"),
            ('s', b"/shm-a"),
        ] {
            let m = Mem::new(&[(name, &data)], 7);
            let (r, placed) = with_files(arena.pixels(), &m, |t| {
                let mut r = Vec::new();
                let keys = keys.replacen("a=T,", &format!("a=T,t={medium},"), 1);
                feed(t, &apc(&keys, &b64(name)), &mut r);
                feed(t, then, &mut r);
                settle(t, &mut r);
                (r, t.images().placements().count())
            });
            assert_eq!(
                r,
                FILE_ERR,
                "{what} t={medium}: {:?}",
                String::from_utf8_lossy(&r)
            );
            assert_eq!(placed, 0, "{what} t={medium}");
            assert_eq!(m.asked().len(), 1, "{what} t={medium}: it was read");
        }
    }
}

/// **A refusal decided before the read keeps its code**, and the reader
/// is never asked: the control data and the path alone decide it, so it
/// says nothing of any file -- `s=0`, `s * v` past the arena, a PNG through
/// zlib with no `S` or one past the scratch, and a path past 2048 bytes.
#[test]
fn a_refusal_before_the_read_keeps_its_code_and_reads_nothing() {
    let long = vec![b'a'; 2049];
    for (keys, path, code) in [
        ("a=T,i=1,t=f,f=24,s=0,v=2", &b"/p"[..], "EINVAL:Zero width"),
        ("a=T,i=1,t=t,f=24,s=9,v=8", b"/p", "EINVAL:Image too large"),
        ("a=T,i=1,t=s,f=100,o=z", b"/p", "ENODATA"),
        (
            "a=T,i=1,t=f,f=100,o=z,S=99999",
            b"/p",
            "EINVAL:PNG data size",
        ),
        (
            "a=T,i=1,t=f,f=24,s=1,v=1",
            &long,
            "EINVAL:Filename too long",
        ),
        (
            "a=T,i=1,I=2,t=f,f=24,s=1,v=1",
            b"/p",
            "EINVAL:Must not specify both",
        ),
    ] {
        let m = Mem::new(&[(b"/p", &[1u8, 2, 3])], 7);
        let r = with_files(64, &m, |t| {
            let mut r = Vec::new();
            feed(t, &apc(keys, &b64(path)), &mut r);
            settle(t, &mut r);
            String::from_utf8_lossy(&r).into_owned()
        });
        assert!(r.contains(&format!(";{code}")), "{keys}: {r:?}");
        assert_eq!(r.matches("\x1b_G").count(), 1, "{keys}: one reply");
        assert_eq!(m.asked().len(), 0, "{keys}: the reader was not asked");
    }
}

/// **The one reply after the read is the same however the reads fall**:
/// three file commands that fail after their reads, each answered EBADF,
/// at every read size and every two-read split.
#[test]
fn the_reply_after_the_read_is_the_same_however_the_reads_fall() {
    let small = png(2, 2, png::ColorType::Rgba, &bytes(16, 3), 8);
    let named: [(&[u8], &[u8]); 3] = [
        (b"/a", b"not a png at all"),
        (b"/b", &[0x78, 0x9c, 0xff, 0xff, 0xff, 0xff]),
        (b"/c", &small),
    ];
    let mut s = b"\x1b[3;4H".to_vec();
    s.extend(apc("a=T,i=1,t=f,f=100", &b64(b"/a")));
    s.extend(apc("a=T,i=2,t=s,f=24,s=2,v=2,o=z", &b64(b"/b")));
    s.extend(apc("a=T,i=3,t=t,f=100,P=5", &b64(b"/c")));
    s.extend_from_slice(b"after");
    let run = |pieces: &[&[u8]]| {
        let m = Mem::new(&named, 5);
        with_files(64, &m, |t| {
            let mut r = Vec::new();
            for p in pieces {
                feed(t, p, &mut r);
            }
            settle(t, &mut r);
            state(t, &r)
        })
    };
    let whole = run(&[&s]);
    for i in 1..=3 {
        let want = format!("i={i};EBADF:Failed to read image file");
        assert!(whole.contains(&want), "{want}: {whole}");
    }
    assert_eq!(whole.matches(";E").count(), 3, "no other code: {whole}");
    for n in 1..=s.len() {
        let pieces: Vec<&[u8]> = s.chunks(n).collect();
        assert_eq!(run(&pieces), whole, "read size {n}");
    }
    for cut in 1..s.len() {
        assert_eq!(run(&[&s[..cut], &s[cut..]]), whole, "cut at {cut}");
    }
}

/// `stream` fed in `pieces` to a terminal of `pixels` lent `named`, the
/// decode settled, and the state it leaves.
fn file_state(pixels: usize, named: &[(&[u8], &[u8])], pieces: &[&[u8]]) -> String {
    let m = Mem::new(named, 7);
    with_files(pixels, &m, |t| {
        let mut r = Vec::new();
        for p in pieces {
            feed(t, p, &mut r);
        }
        settle(t, &mut r);
        state(t, &r)
    })
}

/// **A refusal after the read keeps nothing**: an `a=T` through `t=f`,
/// `t=t` and `t=s` whose placement is refused once its file is read -- a
/// relative placement, the store's placements full, the stream's
/// virtual table full -- leaves what a missing file leaves, raw and PNG:
/// the one error, the image gone, so the `a=p` after it answers ENOENT,
/// and the cursor where it was ("all failures to read an image file ...
/// a single, identical, error response", "The transmission medium").
#[test]
fn a_refusal_after_the_read_keeps_nothing() {
    let small = png(2, 2, png::ColorType::Rgba, &bytes(16, 3), 8);
    let raw = bytes(12, 4);
    let raw_z = zlib_mixed(&raw);
    let fill = |placement: &dyn Fn(usize) -> String, n: usize| {
        let mut s = apc("a=t,i=3,f=24,s=1,v=1,q=2", &b64(&[1, 2, 3]));
        for p in 1..=n {
            s.extend(apc(&placement(p), b""));
        }
        s
    };
    let physical = fill(&|p| format!("a=p,i=3,p={p},C=1,q=2"), placement_slots());
    let virtuals = fill(
        &|p| format!("a=p,U=1,i=3,p={p},c=1,r=1,q=2"),
        baud_domain::KITTY_VIRTUALS,
    );
    let cases: [(&str, &[u8], &str, &[u8]); 3] = [
        ("relative", b"", ",P=5", b""),
        (
            "placements",
            &physical,
            ",C=1",
            b"\x1b_Ga=d,d=i,i=3,q=2\x1b\\",
        ),
        (
            "virtuals",
            &virtuals,
            ",U=1,c=1,r=1",
            b"\x1b_Ga=d,d=i,i=3,q=2\x1b\\",
        ),
    ];
    let mut differ = Vec::new();
    for (what, before, extra, after) in cases {
        for (format, data) in [
            ("f=24,s=2,v=2", &raw),
            ("f=24,s=2,v=2,o=z", &raw_z),
            ("f=100", &small),
        ] {
            for (medium, name) in [
                ('f', &b"/pics/a"[..]),
                ('t', b"/tmp/tty-graphics-protocol-a"),
                ('s', b"/shm-a"),
            ] {
                let stream = |path: &[u8]| {
                    let mut s = b"\x1b[3;4H".to_vec();
                    s.extend_from_slice(before);
                    let keys = format!("a=T,i=1,t={medium},{format}{extra}");
                    s.extend(apc(&keys, &b64(path)));
                    s.extend_from_slice(after);
                    s.extend(apc("a=p,i=1", b""));
                    s
                };
                let missing = file_state(64, &[(name, data)], &[&stream(b"/gone")]);
                assert!(
                    missing.contains(
                        "i=1;EBADF:Failed to read image file\\u{1b}\\\\\\u{1b}_Gi=1;ENOENT"
                    ),
                    "{what} {format} t={medium}: the control: {missing}"
                );
                // The embedder's counts differ -- a refused virtual
                // placement here, and outside `state` a refused placement
                // and an image dropped -- and no reply carries any of them.
                let read = file_state(64, &[(name, data)], &[&stream(name)])
                    .replace("virtuals_refused: 1", "virtuals_refused: 0");
                if read != missing {
                    differ.push(format!("{what} {format} t={medium}:\n{read}{missing}"));
                }
            }
        }
    }
    assert!(differ.is_empty(), "{} differ: {differ:#?}", differ.len());
}

/// **A refusal after the read evicts nothing a missing file keeps**: an
/// older image with no placement fills the arena, an `a=T` through `t=f`,
/// `t=t` and `t=s` whose placement is refused once its file is read --
/// relative, the store's placements full, the virtual table full --
/// needs its space, raw or PNG, each direct or through zlib, and the
/// `a=p` of the older image after it answers what it answers after a
/// missing file: `OK` for a PNG, whose size is
/// known only once read, and ENOENT for raw, which reserves, and
/// deletes, before the read ("must not allow such a program to use the
/// responses to these commands to learn anything about files it cannot
/// read itself", "The transmission medium"). The control that the arena
/// is tight: the same PNG placed deletes the older image. A PNG at every
/// read size and every two-read split.
#[test]
fn a_refusal_after_the_read_evicts_nothing_a_missing_file_keeps() {
    let small = png(2, 2, png::ColorType::Rgba, &bytes(16, 3), 8);
    let raw = bytes(12, 4);
    let raw_z = zlib_mixed(&raw);
    let small_z = zlib_mixed(&small);
    let png_z = format!("f=100,o=z,S={}", small.len());
    // Image 7, 63 of the arena's 64 pixels, beside image 3's one.
    let older = apc("a=t,i=7,f=24,s=7,v=9,q=2", &b64(&bytes(7 * 9 * 3, 5)));
    let fill = |placement: &dyn Fn(usize) -> String, n: usize| {
        let mut s = apc("a=t,i=3,f=24,s=1,v=1,q=2", &b64(&[1, 2, 3]));
        for p in 1..=n {
            s.extend(apc(&placement(p), b""));
        }
        s
    };
    let physical = fill(&|p| format!("a=p,i=3,p={p},C=1,q=2"), placement_slots());
    let virtuals = fill(
        &|p| format!("a=p,U=1,i=3,p={p},c=1,r=1,q=2"),
        baud_domain::KITTY_VIRTUALS,
    );
    let undo = b"\x1b_Ga=d,d=i,i=3,q=2\x1b\\";
    let cases: [(&str, &[u8], &str, &[u8]); 3] = [
        ("relative", b"", ",P=5", b""),
        ("placements", &physical, ",C=1", undo),
        ("virtuals", &virtuals, ",U=1,c=1,r=1", undo),
    ];
    let mediums: [(char, &[u8]); 3] = [
        ('f', b"/pics/a"),
        ('t', b"/tmp/tty-graphics-protocol-a"),
        ('s', b"/shm-a"),
    ];
    let stream = |before: &[u8], keys: &str, path: &[u8], after: &[u8]| {
        let mut s = b"\x1b[3;4H".to_vec();
        s.extend_from_slice(before);
        s.extend_from_slice(&older);
        s.extend(apc(keys, &b64(path)));
        s.extend_from_slice(after);
        s.extend(apc("a=p,i=7", b""));
        s
    };
    for (medium, name) in mediums {
        let placed = file_state(
            64,
            &[(name, &small)],
            &[&stream(
                b"",
                &format!("a=T,i=1,t={medium},f=100"),
                name,
                b"",
            )],
        );
        assert!(
            placed.contains("i=1;OK\\u{1b}\\\\\\u{1b}_Gi=7;ENOENT") && placed.contains("evicted 1"),
            "t={medium}: the arena is tight: {placed}"
        );
    }
    let mut differ = Vec::new();
    for (what, before, extra, after) in cases {
        for (format, data, older_after) in [
            ("f=24,s=2,v=2", &raw, "ENOENT"),
            ("f=24,s=2,v=2,o=z", &raw_z, "ENOENT"),
            ("f=100", &small, "OK"),
            (png_z.as_str(), &small_z, "OK"),
        ] {
            for (medium, name) in mediums {
                let keys = format!("a=T,i=1,t={medium},{format}{extra}");
                let named: [(&[u8], &[u8]); 1] = [(name, data)];
                let missing = file_state(64, &named, &[&stream(before, &keys, b"/gone", after)]);
                let want = format!(
                    "i=1;EBADF:Failed to read image file\\u{{1b}}\\\\\\u{{1b}}_Gi=7;{older_after}"
                );
                assert!(
                    missing.contains(&want),
                    "{what} {format} t={medium}: the control: {missing}"
                );
                let s = stream(before, &keys, name, after);
                let read = |pieces: &[&[u8]]| {
                    file_state(64, &named, pieces)
                        .replace("virtuals_refused: 1", "virtuals_refused: 0")
                };
                if read(&[&s]) != missing {
                    differ.push(format!(
                        "{what} {format} t={medium}:\n{}{missing}",
                        read(&[&s])
                    ));
                    continue;
                }
                if what != "relative" || format != "f=100" {
                    continue;
                }
                for n in 1..=s.len() {
                    let pieces: Vec<&[u8]> = s.chunks(n).collect();
                    assert_eq!(read(&pieces), missing, "t={medium} read size {n}");
                }
                for cut in 1..s.len() {
                    assert_eq!(
                        read(&[&s[..cut], &s[cut..]]),
                        missing,
                        "t={medium} cut at {cut}"
                    );
                }
            }
        }
    }
    assert!(differ.is_empty(), "{} differ: {differ:#?}", differ.len());
}

/// CRC-32 of `bytes` (ISO 3309, PNG's), bit by bit: what a hand-built
/// chunk carries.
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 == 1 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// A PNG built by hand around `stream`: `IHDR` for a `w` x `h` image of
/// colour type `color` at `depth` bits, a `PLTE` of `plte` if any, the
/// stream cut into `IDAT` chunks of 50 bytes, every CRC right.
fn png_around(w: u32, h: u32, color: u8, depth: u8, plte: &[u8], stream: &[u8]) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = w.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[depth, color, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    if !plte.is_empty() {
        chunk(b"PLTE", plte);
    }
    for part in stream.chunks(50) {
        chunk(b"IDAT", part);
    }
    chunk(b"IEND", &[]);
    out
}

/// **A file PNG that does not decode deletes nothing for space**: image 7
/// takes 3614 of the arena's 4096 pixels with no placement, and a 21x23
/// PNG read through `t=f`, `t=t` or `t=s`, direct or through zlib, sent
/// `a=t` or `a=T`, whose header parses and whose data does not decode --
/// a later `IDAT`'s CRC, a reserved block type, the Adler-32, the stream
/// ending inside the `IDAT` run, a filter type past 4, a palette index
/// past `PLTE` at 8 bits and at 2 -- leaves what a missing file leaves:
/// the one error, and the `a=p` of image 7 after it answered `OK`. The
/// image was never added, so nothing was deleted "to make space for the
/// new one" ("Image persistence and storage quotas"), and what a file
/// holds is not told by what it deletes ("The transmission medium"). The
/// control that the arena is tight: the same PNG whole deletes image 7.
/// The probe at every read size and every two-read split.
#[test]
fn a_file_png_that_does_not_decode_deletes_nothing_for_space() {
    let (w, h) = (21u32, 23u32);
    let good = png(w, h, png::ColorType::Rgba, &bytes(21 * 23 * 4, 7), 50);
    let mut late = good.clone();
    let at = late.len() - 12 - 4 - 10;
    late[at] ^= 0xFF;
    // Scanlines of filter type 0, and the same with one row's past 4.
    let rgba_rows: Vec<u8> = (0..23)
        .flat_map(|r| {
            let mut row = vec![0u8];
            row.extend(bytes(21 * 4, 40 + r));
            row
        })
        .collect();
    let mut bad_filter = rgba_rows.clone();
    bad_filter[11 * (1 + 21 * 4)] = 5;
    let z = zlib_mixed(&rgba_rows);
    let mut adler = z.clone();
    *adler.last_mut().unwrap() ^= 0x01;
    let short = z[..z.len() / 2].to_vec();
    // Two palette entries, and one pixel naming a third.
    let plte = [255, 0, 0, 0, 0, 255];
    let mut index8: Vec<u8> = (0..23)
        .flat_map(|_| [vec![0u8], vec![1u8; 21]].concat())
        .collect();
    index8[20 * 22 + 5] = 2;
    // Two bits a pixel, six bytes a row; index 3 at one pixel.
    let mut index2: Vec<u8> = (0..23)
        .flat_map(|_| [0u8, 0x55, 0x55, 0x55, 0x55, 0x55, 0x40])
        .collect();
    index2[7 * 7 + 3] = 0x5D;
    let broken: [(&str, Vec<u8>); 7] = [
        ("a later IDAT's CRC", late),
        (
            "a reserved block type",
            png_around(w, h, 6, 8, &[], &[0x78, 0x01, 0x07, 0, 0, 0, 0]),
        ),
        ("the Adler-32", png_around(w, h, 6, 8, &[], &adler)),
        (
            "the stream ending inside the IDAT run",
            png_around(w, h, 6, 8, &[], &short),
        ),
        (
            "a filter type past 4",
            png_around(w, h, 6, 8, &[], &zlib_mixed(&bad_filter)),
        ),
        (
            "a palette index past PLTE at 8 bits",
            png_around(w, h, 3, 8, &plte, &zlib_mixed(&index8)),
        ),
        (
            "a palette index past PLTE at 2 bits",
            png_around(w, h, 3, 2, &plte, &zlib_mixed(&index2)),
        ),
    ];
    // Image 7, 3614 pixels, beside which 482 are free.
    let older = apc(
        "a=t,i=7,f=24,s=139,v=26,o=z,q=2",
        &b64(&zlib_mixed(&vec![0u8; 139 * 26 * 3])),
    );
    let mediums: [(char, &[u8]); 3] = [
        ('f', b"/pics/a"),
        ('t', b"/tmp/tty-graphics-protocol-a"),
        ('s', b"/shm-a"),
    ];
    let stream = |keys: &str, path: &[u8]| {
        let mut s = b"\x1b[3;4H".to_vec();
        s.extend_from_slice(&older);
        s.extend(apc(keys, &b64(path)));
        s.extend(apc("a=p,i=7", b""));
        s
    };
    let zipped = |data: &[u8]| (format!("f=100,o=z,S={}", data.len()), zlib_mixed(data));
    for (medium, name) in mediums {
        for act in ['t', 'T'] {
            for (format, data) in [("f=100".to_string(), good.clone()), zipped(&good)] {
                let keys = format!("a={act},i=1,t={medium},{format}");
                let placed = file_state(4096, &[(name, &data)], &[&stream(&keys, name)]);
                assert!(
                    placed.contains("i=1;OK\\u{1b}\\\\\\u{1b}_Gi=7;ENOENT")
                        && placed.contains("evicted 1"),
                    "a={act} {format} t={medium}: the arena is tight: {placed}"
                );
            }
        }
    }
    let mut differ = Vec::new();
    for (what, data) in &broken {
        assert!(
            baud_domain::png::Png::parse(data).is_ok(),
            "{what}: the header parses"
        );
        for (medium, name) in mediums {
            for act in ['t', 'T'] {
                for (format, data) in [("f=100".to_string(), data.clone()), zipped(data)] {
                    let keys = format!("a={act},i=1,t={medium},{format}");
                    let named: [(&[u8], &[u8]); 1] = [(name, &data)];
                    let missing = file_state(4096, &named, &[&stream(&keys, b"/gone")]);
                    assert!(
                        missing.contains(
                            "i=1;EBADF:Failed to read image file\\u{1b}\\\\\\u{1b}_Gi=7;OK"
                        ),
                        "{what} a={act} {format} t={medium}: the control: {missing}"
                    );
                    let read = file_state(4096, &named, &[&stream(&keys, name)]);
                    if read != missing {
                        let tail = |s: &str| s.lines().last().unwrap_or("").to_string();
                        differ.push(format!(
                            "{what} a={act} {format} t={medium}:\n{}\n{}",
                            tail(&read),
                            tail(&missing)
                        ));
                    }
                }
            }
        }
    }
    assert!(differ.is_empty(), "{} differ: {differ:#?}", differ.len());
    let (_, late) = &broken[0];
    let named: [(&[u8], &[u8]); 1] = [(b"/pics/a", late)];
    let s = stream("a=T,i=1,t=f,f=100", b"/pics/a");
    let missing = file_state(4096, &named, &[&stream("a=T,i=1,t=f,f=100", b"/gone")]);
    for n in 1..=s.len() {
        let pieces: Vec<&[u8]> = s.chunks(n).collect();
        assert_eq!(file_state(4096, &named, &pieces), missing, "read size {n}");
    }
    for cut in 1..s.len() {
        assert_eq!(
            file_state(4096, &named, &[&s[..cut], &s[cut..]]),
            missing,
            "cut at {cut}"
        );
    }
}

/// **A virtual placement whose image was freed is room for a file PNG's**:
/// the stream's table filled but for one by image 3's placements, the last
/// slot by a `t=d` PNG placed `U=1` whose decode then failed, freeing its
/// image under the placement, and a file PNG placed `U=1` after them is
/// answered `OK`, as `place` makes room by dropping what names no live
/// image. The control: the last slot's PNG decoded, the table is full and
/// the file PNG is the one error.
#[test]
fn a_virtual_naming_a_freed_image_is_room_for_a_file_png() {
    let good = png(21, 23, png::ColorType::Rgba, &bytes(21 * 23 * 4, 7), 50);
    let mut late = good.clone();
    let at = late.len() - 12 - 4 - 10;
    late[at] ^= 0xFF;
    let small = png(2, 2, png::ColorType::Rgba, &bytes(16, 3), 8);
    let run = |last: &[u8]| {
        let m = Mem::new(&[(b"/pics/a", &small)], 7);
        with_files(4096, &m, |t| {
            let mut r = Vec::new();
            feed(
                t,
                &apc("a=t,i=3,f=24,s=1,v=1,q=2", &b64(&[1, 2, 3])),
                &mut r,
            );
            for p in 1..baud_domain::KITTY_VIRTUALS {
                let keys = format!("a=p,U=1,i=3,p={p},c=1,r=1,q=2");
                feed(t, &apc(&keys, b""), &mut r);
            }
            feed(t, &apc("a=T,U=1,i=9,f=100,c=1,r=1", &b64(last)), &mut r);
            settle(t, &mut r);
            let first = core::mem::take(&mut r);
            let keys = "a=T,U=1,i=1,t=f,f=100,c=1,r=1";
            feed(t, &apc(keys, &b64(b"/pics/a")), &mut r);
            settle(t, &mut r);
            // An image no id names is given no virtual placement, full
            // table or not, and is kept, as `place` keeps it.
            let images = t.kitty().images;
            feed(
                t,
                &apc("a=T,U=1,t=f,f=100,c=1,r=1", &b64(b"/pics/a")),
                &mut r,
            );
            settle(t, &mut r);
            assert_eq!(t.kitty().images, images + 1, "the nameless image is kept");
            (first, r)
        })
    };
    let live = run(&good);
    assert_eq!(live.0, b"\x1b_Gi=9;OK\x1b\\", "the control: placed");
    assert_eq!(live.1, FILE_ERR, "the control: the table is full");
    let dead = run(&late);
    assert!(
        dead.0.starts_with(b"\x1b_Gi=9;EBADPNG:"),
        "the last slot's image died"
    );
    assert_eq!(dead.1, b"\x1b_Gi=1;OK\x1b\\");
}

/// **A file PNG that fails late moves nothing and answers before the next
/// byte**: a PNG whose header and first `IDAT` parse and whose last does
/// not, through `t=f`, `t=t` and `t=s`, then a CPR and a DA, leaves the
/// bytes and the state a missing file leaves -- the one error, then the
/// cursor where it was, then the DA, then ENOENT for the `a=p` naming the
/// image -- at every read size and every two-read split.
#[test]
fn a_file_png_that_fails_late_moves_nothing_and_answers_in_order() {
    let mut late = png(21, 23, png::ColorType::Rgba, &bytes(21 * 23 * 4, 7), 50);
    let at = late.len() - 12 - 4 - 10;
    late[at] ^= 0xFF;
    for (medium, name) in [
        ('f', &b"/pics/a"[..]),
        ('t', b"/tmp/tty-graphics-protocol-a"),
        ('s', b"/shm-a"),
    ] {
        let named: [(&[u8], &[u8]); 1] = [(name, &late)];
        let stream = |path: &[u8]| {
            let mut s = b"\x1b[3;4H".to_vec();
            s.extend(apc(&format!("a=T,i=1,t={medium},f=100"), &b64(path)));
            s.extend_from_slice(b"\x1b[6n\x1b[c");
            s.extend(apc("a=p,i=1", b""));
            s
        };
        let missing = file_state(4096, &named, &[&stream(b"/gone")]);
        assert!(
            missing.contains("EBADF:Failed to read image file\\u{1b}\\\\\\u{1b}[3;4R\\u{1b}[?")
                && missing.contains("c\\u{1b}_Gi=1;ENOENT"),
            "t={medium}: the control: {missing}"
        );
        let s = stream(name);
        assert_eq!(file_state(4096, &named, &[&s]), missing, "t={medium}");
        for n in 1..=s.len() {
            let pieces: Vec<&[u8]> = s.chunks(n).collect();
            let got = file_state(4096, &named, &pieces);
            assert_eq!(got, missing, "t={medium} read size {n}");
        }
        for cut in 1..s.len() {
            let got = file_state(4096, &named, &[&s[..cut], &s[cut..]]);
            assert_eq!(got, missing, "t={medium} cut at {cut}");
        }
    }
}

/// **A file PNG is decoded whole at its terminator**: an `ED 2` straight
/// after it clears a picture already shown, and the command was answered
/// `OK` before it, where `t=d`'s deferred decode is cleared mid-way and
/// answered ENOENT.
#[test]
fn a_file_png_is_decoded_whole_at_its_terminator() {
    let p = png(21, 23, png::ColorType::Rgba, &bytes(21 * 23 * 4, 91), 50);
    let direct = with(4096, |t| {
        let mut r = Vec::new();
        feed(t, &apc("a=T,i=1,f=100", &b64(&p)), &mut r);
        feed(t, b"\x1b[2J", &mut r);
        settle(t, &mut r);
        r
    });
    assert!(
        direct.starts_with(b"\x1b_Gi=1;ENOENT:Image deleted"),
        "{direct:?}"
    );
    for (medium, name) in [
        ('f', &b"/pics/a"[..]),
        ('t', b"/tmp/tty-graphics-protocol-a"),
        ('s', b"/shm-a"),
    ] {
        let m = Mem::new(&[(name, &p)], 7);
        let r = with_files(4096, &m, |t| {
            let mut r = Vec::new();
            let keys = format!("a=T,i=1,t={medium},f=100");
            feed(t, &apc(&keys, &b64(name)), &mut r);
            assert_eq!(t.decode_step(DecodeBudget::ALL), DecodeProgress::Idle);
            feed(t, b"\x1b[2J", &mut r);
            settle(t, &mut r);
            r
        });
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\", "t={medium}");
    }
}

/// **`ED 3` takes a picture with the history only when all of it was
/// there**: erasing the saved lines makes the screen's top the oldest
/// retained line, so a four-row picture two rows into the history and
/// two on screen keeps its rows on screen, and one wholly in the history
/// goes, counted.
#[test]
fn erasing_the_history_keeps_a_picture_with_rows_on_screen() {
    for (lfs, kept) in [(2, true), (5, false)] {
        with_history(4096, |t| {
            let mut r = Vec::new();
            feed(
                t,
                &apc("a=T,f=24,s=10,v=40,i=1,r=4,q=2,C=1", &b64(&[7u8; 1200])),
                &mut r,
            );
            feed(t, b"\x1b[24;1H", &mut r);
            for _ in 0..lfs {
                feed(t, b"\n", &mut r);
            }
            assert_eq!(
                kitty_lines(t),
                vec![0],
                "{lfs}: the premise, in the history"
            );
            feed(t, b"\x1b[3J", &mut r);
            assert_eq!(
                oldest(t),
                u64::try_from(lfs).unwrap(),
                "{lfs}: the history erased"
            );
            match kept {
                true => assert_eq!(kitty_lines(t), vec![0], "{lfs}: rows on screen, kept"),
                false => {
                    assert_eq!(kitty_lines(t), Vec::<u64>::new(), "{lfs}: gone");
                    assert_eq!(t.images().placements_expired, 1, "{lfs}: counted");
                }
            }
        });
    }
}

/// **A reflow keeps a picture whose anchor is past the history's far end
/// at its place, and drops it with its rows**: the reflow re-cuts the
/// lines from the oldest retained one, which it does not move, so a
/// placement anchored above that line keeps its distance from it. Under
/// short lines a narrower screen re-cuts nothing and the picture's last
/// two rows stay over the two oldest lines; under long ones each line
/// takes two rows, the ring drops the oldest, and the picture goes with
/// the rows it was over, counted.
#[test]
fn a_reflow_keeps_a_picture_past_the_far_end_and_drops_it_with_its_rows() {
    for (text, kept) in [("short", true), ("long line ".repeat(7).as_str(), false)] {
        with_history(4096, |t| {
            let mut r = Vec::new();
            feed(t, b"\x1b[3;1H", &mut r);
            feed(
                t,
                &apc("a=T,f=24,s=10,v=40,i=1,r=4,q=2,C=1", &b64(&[7u8; 1200])),
                &mut r,
            );
            for i in 0..24 {
                feed(
                    t,
                    format!("\x1b[{};1H{i:02} {text}", i + 1).as_bytes(),
                    &mut r,
                );
            }
            scroll_until(t, 4, &mut r);
            assert_eq!(
                kitty_lines(t),
                vec![2],
                "the premise: its last two rows kept"
            );
            let ws = WinSize {
                ws_row: 24,
                ws_col: 40,
            };
            assert_eq!(t.resize(&ws), Resized::Ok);
            match kept {
                true => {
                    assert_eq!(kitty_lines(t), vec![2], "{text}: kept at its line");
                    assert_eq!(oldest(t), 4, "{text}: the oldest line did not move");
                }
                false => {
                    assert_eq!(kitty_lines(t), Vec::<u64>::new(), "{text}: gone");
                    assert_eq!(t.images().placements_expired, 1, "{text}: counted");
                }
            }
        });
    }
}
