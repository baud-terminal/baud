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
    DecodeBudget, DecodeProgress, ImageAlpha, ImageBudget, ImagePx, KittyPlacement, LineIdx, Pixel,
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
fn chunked(first: &str, rest: &str, encoded: &[u8], n: usize) -> Vec<u8> {
    let pieces: Vec<&[u8]> = encoded.chunks(n).collect();
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
        e.set_adaptive_filter(png::AdaptiveFilterType::Adaptive);
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
        // goes right one and down one (`[[source: kitty-graphics]]`).
        assert_eq!(t.cursor(), (Row::from_index(5), Col::at_index(7)));
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
        assert_eq!(
            t.cursor(),
            (Row::from_index(2), Col::at_index(3)),
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
            b"\x1b_Gi=4;ENODATA:Insufficient image data\x1b\\",
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

#[test]
fn a_cancelled_chunk_aborts_its_upload_and_is_counted() {
    let data = b64(&[7u8; 12]);
    let mut s = apc("a=T,f=24,s=2,v=2,i=1,m=1", &data[..8]);
    s.extend_from_slice(b"\x1b_Gm=0;");
    s.extend_from_slice(&data[8..12]);
    s.push(0x18);
    s.extend(apc("a=T,f=24,s=1,v=1,i=2", &b64(&[1, 2, 3])));
    with(64, |t| {
        let mut r = Vec::new();
        feed(t, &s, &mut r);
        assert_eq!(t.kitty().cancelled, 1);
        assert_eq!(r, b"\x1b_Gi=2;OK\x1b\\", "the next command is its own");
        assert_eq!(t.images().placements().count(), 1);
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

#[test]
fn placeholders_relative_placements_and_animation_are_refused_or_ignored() {
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
        assert_eq!(
            r,
            "\x1b_Gi=1;EINVAL:Unicode placeholder placements are not supported\x1b\\\
             \x1b_Gi=1;EINVAL:Relative placements are not supported\x1b\\"
        );
        assert_eq!(t.images().placements().count(), 0);
        assert_eq!(t.cursor(), (Row::ZERO, Col::ZERO), "no cursor move");
        assert_eq!(t.kitty().ignored, 1, "the frame, counted");
        // The image itself was stored by the a=T.
        let mut r = Vec::new();
        feed(t, &apc("a=p,i=1", b""), &mut r);
        assert_eq!(r, b"\x1b_Gi=1;OK\x1b\\");
    });
}

/// **A reply is `ESC _ G ... ESC \\` whatever S8C1T says**: the shape the
/// text gives ("`<ESC>_Gi=31;OK<ESC>\\`"). Following S8C1T is an open owner
/// question (Jev, FOLLOW_S8C1T 0.92 and 0.88, under the bar). The DA after
/// it is the control: S8C1T took.
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

/// An image with a placement is never deleted for space, however old.
#[test]
fn a_placed_image_is_never_deleted_to_make_space() {
    with(12, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('T', 1), &mut r);
        for i in 2..=4 {
            feed(t, &tiny('t', i), &mut r);
        }
        assert!(r.ends_with(b"\x1b_Gi=4;OK\x1b\\"), "{r:?}");
        assert_eq!(t.images().images_evicted, 1);
        assert_eq!(t.images().placements().count(), 1, "still placed");
        assert_eq!(live(t, 4), [1, 3, 4], "the oldest unplaced went");
    });
}

/// When deleting every unplaced image would not make space, none is
/// deleted: the new image is refused with ENOSPC and counted once.
#[test]
fn with_nothing_to_delete_the_new_image_is_refused_and_counted_once() {
    with(8, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('T', 1), &mut r);
        feed(t, &tiny('T', 2), &mut r);
        r.clear();
        feed(t, &tiny('t', 3), &mut r);
        assert!(r.starts_with(b"\x1b_Gi=3;ENOSPC:"), "{r:?}");
        let imgs = t.images();
        assert_eq!((imgs.images_evicted, imgs.images_refused), (0, 1));
        assert_eq!(imgs.placements().count(), 2);
    });
    // Deleting the unplaced image would leave gaps of 4 and 2 pixels
    // between the placed ones, none the 6 a 3x2 image needs: it stays.
    with(14, |t| {
        let mut r = Vec::new();
        feed(t, &tiny('T', 1), &mut r);
        feed(t, &tiny('t', 2), &mut r);
        feed(t, &tiny('T', 3), &mut r);
        r.clear();
        let wide = apc("a=t,f=32,s=3,v=2,i=4", &b64(&bytes(24, 4)));
        feed(t, &wide, &mut r);
        assert!(r.starts_with(b"\x1b_Gi=4;ENOSPC:"), "{r:?}");
        assert_eq!(t.images().images_evicted, 0);
        assert_eq!(live(t, 3), [1, 2, 3], "nothing deleted for nothing");
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
/// aspect, so twelve pixels high.
const SIXEL: &[u8] = b"\x1bPq#0;2;100;0;0#0~~\x1b\\";

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
