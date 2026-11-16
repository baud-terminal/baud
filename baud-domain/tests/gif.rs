//! The GIF decoder against `[[source: gif89a]]`.
//!
//! **A real encoder's files, and this file's own.** ImageMagick 7.1.2's
//! encoder wrote three files whose images cover their logical screens --
//! 256 colours of noise, whose table fills and clears; an interlaced
//! scene; a two-colour one at the minimum code size of 2 -- and its own
//! decode of each is the reference (`baud-domain/tests/gif/README.md`):
//! GIF is lossless, so any decoder that follows Appendix F gets the same
//! indices. The rest are written here by an LZW encoder of Appendix F's
//! own, independent of the decoder, so each block's meaning can be set
//! case by case: the background colour, the disposals, transparency,
//! local tables, interlace, zero delays, the loop count, the User Input
//! Flag, a Plain Text Extension drawn through a stub font, a stream cut
//! short.
//!
//! **Malformed input never panics.** The property harms valid streams by
//! named classes -- a minimum code size outside 2 to 8, a table filled
//! with no Clear, a first code that is no Clear, an image past the
//! logical screen, a sub-block cut short, a size at 65535, the stream cut
//! at every block -- and a positive control requires each to fire.

use baud_domain::gif::{Gif, GifError, GifWait};
use baud_domain::{
    Animated, CellFont, FileLen, FrameNo, HostTime, ImageId, ImageMotion, ImagePx, ImageSlot,
    ImageStore, Pixel, PlacementId, PlacementSlot, decode_picture,
};
use proptest::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;

/// How an encoder meets a full table.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Full {
    /// It sends a Clear.
    Clear,
    /// It keeps sending codes with the table full, "deferred clear".
    Defer,
}

/// **Appendix F's compressor**: the Clear code first, the string table
/// grown a code at a time, each code `width` bits written right to left,
/// the width one more once the table's next code would not fit, at most
/// twelve; the End of Information code last.
fn lzw(indices: &[u8], m: u8, full: Full) -> Vec<u8> {
    let clear: u16 = 1 << m;
    let eoi = clear + 1;
    let mut out = Vec::new();
    let (mut acc, mut n) = (0u32, 0u32);
    let mut width = u32::from(m) + 1;
    let mut put = |c: u16, width: u32| {
        acc |= u32::from(c) << n;
        n += width;
        while n >= 8 {
            out.push(acc.to_le_bytes()[0]);
            acc >>= 8;
            n -= 8;
        }
    };
    put(clear, width);
    let mut dict: HashMap<(u16, u8), u16> = HashMap::new();
    let mut next = clear + 2;
    let mut w = u16::from(indices[0]);
    for &k in &indices[1..] {
        if let Some(&c) = dict.get(&(w, k)) {
            w = c;
            continue;
        }
        put(w, width);
        if next < 4096 {
            dict.insert((w, k), next);
            next += 1;
            if u32::from(next) > 1 << width && width < 12 {
                width += 1;
            }
        } else if full == Full::Clear {
            put(clear, width);
            dict.clear();
            next = clear + 2;
            width = u32::from(m) + 1;
        }
        w = u16::from(k);
    }
    put(w, width);
    put(eoi, width);
    if n > 0 {
        out.push(acc.to_le_bytes()[0]);
    }
    out
}

/// Data in sub-blocks of at most 255 bytes and the terminator.
fn blocks(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for c in data.chunks(255) {
        out.push(u8::try_from(c.len()).unwrap());
        out.extend_from_slice(c);
    }
    out.push(0);
    out
}

/// One graphic of a stream this file writes.
#[derive(Clone, Default)]
struct Img {
    rect: [u16; 4],
    px: Vec<u8>,
    /// (disposal, delay, transparency index, user input).
    gce: Option<(u8, u16, Option<u8>, bool)>,
    local: Option<Vec<[u8; 3]>>,
    rows: Rows,
}

/// Whether an image is written in Appendix E's passes.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
enum Rows {
    #[default]
    InOrder,
    Interlaced,
}

/// A colour table padded to a power of two, and its size field.
fn palette(t: &[[u8; 3]]) -> (Vec<u8>, u8) {
    let bits = (t.len().max(2) - 1).ilog2() + 1;
    let size = 1usize << bits;
    let mut out: Vec<u8> = t.iter().flatten().copied().collect();
    out.resize(size * 3, 0);
    (out, u8::try_from(bits - 1).unwrap())
}

const K: [u8; 3] = [0, 0, 0];
const W: [u8; 3] = [255, 255, 255];
const R: [u8; 3] = [255, 0, 0];
const G: [u8; 3] = [0, 255, 0];
const B: [u8; 3] = [0, 0, 255];

/// **A GIF89a stream**: the screen `w` by `h`, the global table and
/// background index, a NETSCAPE2.0 block with `loops` if given, the images.
fn gif(w: u16, h: u16, gct: &[[u8; 3]], bg: u8, loops: Option<u16>, images: &[Img]) -> Vec<u8> {
    let mut out = b"GIF89a".to_vec();
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    let (table, size) = palette(gct);
    out.extend([0x80 | 0x70 | size, bg, 0]);
    out.extend(table);
    if let Some(n) = loops {
        out.extend(b"\x21\xff\x0bNETSCAPE2.0\x03\x01");
        out.extend(n.to_le_bytes());
        out.push(0);
    }
    for im in images {
        if let Some((disposal, delay, t, input)) = im.gce {
            let flags = disposal << 2 | u8::from(input) << 1 | u8::from(t.is_some());
            out.extend([0x21, 0xf9, 4, flags]);
            out.extend(delay.to_le_bytes());
            out.extend([t.unwrap_or(0), 0]);
        }
        out.push(0x2c);
        for v in im.rect {
            out.extend(v.to_le_bytes());
        }
        let local = im.local.as_ref().map(|t| palette(t));
        let flags = local.as_ref().map_or(0, |(_, s)| 0x80 | s)
            | u8::from(im.rows == Rows::Interlaced) << 6;
        out.push(flags);
        if let Some((t, _)) = &local {
            out.extend(t);
        }
        let m = 2.max(local.as_ref().map_or(size, |(_, s)| *s) + 1);
        out.push(m);
        out.extend(blocks(&lzw(&im.px, m, Full::Clear)));
    }
    out.push(0x3b);
    out
}

/// Every frame `g` decodes to: its RGBA bytes, gap and wait.
fn frames(g: &[u8]) -> Result<Vec<(Vec<u8>, u32, GifWait)>, GifError> {
    frames_with(g, None)
}

/// [`frames`], its Plain Text Extensions drawn with `font`.
fn frames_with(g: &[u8], font: Option<CellFont>) -> Result<Vec<(Vec<u8>, u32, GifWait)>, GifError> {
    let gif = Gif::parse(g)?.with_font(font);
    let mut scratch = vec![0x5au8; gif.scratch_len()];
    let mut out = Vec::new();
    gif.decode(&mut scratch, |f| {
        out.push((f.rgba.to_vec(), f.gap_ms, f.wait));
        true
    })?;
    assert_eq!(
        u32::try_from(out.len()).unwrap(),
        gif.frames(),
        "the frames parse counted"
    );
    Ok(out)
}

/// **`g` stored as an inline picture is** ([`decode_picture`]), each frame
/// the rectangle in which it differs from the one before, and handed to
/// `f` with the store and its image.
fn with_stored<R>(g: &[u8], f: impl FnOnce(&mut ImageStore<'_>, ImageId) -> R) -> R {
    let gif = Gif::parse(g).unwrap();
    let mut px = vec![Pixel::CLEAR; 1 << 16];
    let mut slots = [ImageSlot::EMPTY; 4];
    let mut places = PlacementSlot::zeroed_vec(4);
    let mut index = PlacementId::zeroed_index(4);
    let mut store = ImageStore::new(&mut px, &mut slots, &mut places, &mut index);
    let mut scratch = g.to_vec();
    scratch.resize(g.len() + gif.scratch_len(), 0);
    let (picture, _) = decode_picture(&mut store, &mut scratch, FileLen::of(g.len()), None);
    let id = picture.expect("stored").id;
    f(&mut store, id)
}

/// The canvas as RGBA bytes.
fn canvas(store: &ImageStore<'_>, id: ImageId) -> Vec<u8> {
    let v = store.image(id).expect("shown");
    v.pixels
        .iter()
        .flat_map(|p| {
            let [a, r, g, b] = p.bits().to_be_bytes();
            [r, g, b, a]
        })
        .collect()
}

/// **The stored path shows what the decoder makes**: every frame of `g`
/// made the one to show -- in order, then back from the last, which
/// replays from the first, and in order again -- composed into the
/// canvas, which is that frame's decode byte for byte. Fails if the
/// decoder's changed box left out a disposal's rectangle, or a record
/// over no frame were applied without its ground.
fn stored_is_decoded(g: &[u8], name: &str) {
    let want = frames(g).unwrap();
    let n = want.len();
    let order: Vec<usize> = (0..n).chain((0..n).rev()).chain(0..n).collect();
    with_stored(g, |store, id| {
        for (step, &k) in order.iter().enumerate() {
            assert!(store.show_frame(id, FrameNo::of(u32::try_from(k).unwrap() + 1)));
            store.compose_frames();
            let v = store.image(id).unwrap();
            assert_eq!(
                v.frame,
                FrameNo::of(u32::try_from(k).unwrap() + 1),
                "{name}"
            );
            assert!(
                canvas(store, id) == want[k].0,
                "{name}: frame {k}, step {step}, stored against decoded"
            );
        }
    });
}

/// RGBA of opaque colours.
fn rgba(px: &[[u8; 3]]) -> Vec<u8> {
    px.iter().flat_map(|c| [c[0], c[1], c[2], 255]).collect()
}

/// **A real encoder's streams decode to its own decode**: a table that
/// fills and clears, interlace, the minimum code size of 2.
#[test]
fn imagemagicks_streams_decode_to_its_reference() {
    let cases: [(&str, &[u8], &[u8]); 3] = [
        (
            "noise256",
            include_bytes!("gif/noise256.gif"),
            include_bytes!("gif/noise256.rgba"),
        ),
        (
            "scene64i",
            include_bytes!("gif/scene64i.gif"),
            include_bytes!("gif/scene64i.rgba"),
        ),
        (
            "scene2",
            include_bytes!("gif/scene2.gif"),
            include_bytes!("gif/scene2.rgba"),
        ),
    ];
    for (name, g, want) in cases {
        let f = frames(g).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(f.len(), 1, "{name}: one frame");
        let wrong = f[0].0.iter().zip(want).filter(|(a, b)| a != b).count();
        assert_eq!(wrong, 0, "{name}: bytes that differ");
    }
}

/// **Codes past twelve bits' table**: the encoder keeps sending with the
/// table full ("deferred clear"), which adds no entry, and both a Clear at
/// full and the deferred form decode to the indices sent.
#[test]
fn a_full_table_decodes_with_and_without_a_clear() {
    // A long run of noise, more than 4096 strings.
    let mut x = 7u32;
    let px: Vec<u8> = (0..120 * 100)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x.to_le_bytes()[0] & 15
        })
        .collect();
    let gct: Vec<[u8; 3]> = (0..16u8).map(|i| [i * 16, 255 - i * 16, i]).collect();
    let want = rgba(&px.iter().map(|&i| gct[usize::from(i)]).collect::<Vec<_>>());
    for full in [Full::Clear, Full::Defer] {
        let mut g = gif(120, 100, &gct, 0, None, &[]);
        g.pop();
        g.push(0x2c);
        for v in [0u16, 0, 120, 100] {
            g.extend(v.to_le_bytes());
        }
        g.extend([0, 4]);
        g.extend(blocks(&lzw(&px, 4, full)));
        g.push(0x3b);
        let f = frames(&g).unwrap();
        assert!(f[0].0 == want, "{full:?}: the indices sent");
    }
}

/// **"The Background Color is the color used for those pixels on the
/// screen that are not covered by an image"**, and disposal 2 restores
/// it; with no global table it is clear.
#[test]
fn the_background_colour_shows_where_no_image_covers() {
    let one = |x, y, v, gce| Img {
        rect: [x, y, 1, 1],
        px: vec![v],
        gce: Some(gce),
        ..Img::default()
    };
    // Background index 2, red: a white pixel, disposed to the background,
    // then a green one.
    let g = gif(
        2,
        2,
        &[K, W, R, G],
        2,
        Some(0),
        &[
            one(0, 0, 1, (2, 50, None, false)),
            one(1, 1, 3, (2, 50, None, false)),
        ],
    );
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2);
    assert_eq!(f[0].0, rgba(&[W, R, R, R]), "the screen red round it");
    assert_eq!(f[1].0, rgba(&[R, R, R, G]), "disposal 2 restored red");
    assert_eq!((f[0].1, f[1].1), (500, 500), "50 hundredths");
    stored_is_decoded(&g, "disposal 2");
    // **Transparent pixels over nothing show the background colour**: the
    // index leaves "the corresponding pixel of the display device ... not
    // modified", and the screen there is the Background Color, so a
    // transparent logo is an opaque box of it, where macOS shows clear.
    let logo = Img {
        rect: [0, 0, 2, 1],
        px: vec![0, 1],
        gce: Some((1, 0, Some(0), false)),
        ..Img::default()
    };
    let f = frames(&gif(2, 1, &[K, W, R, G], 2, None, &[logo])).unwrap();
    assert_eq!(f[0].0, rgba(&[R, W]), "red where the logo is transparent");
    // With no global table the background is clear.
    let mut g = b"GIF89a\x02\x00\x02\x00\x00\x00\x00".to_vec();
    g.extend(b"\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x80");
    g.extend([0, 0, 0, 255, 255, 255, 2]);
    g.extend(blocks(&lzw(&[1], 2, Full::Clear)));
    g.push(0x3b);
    let f = frames(&g).unwrap();
    assert_eq!(f[0].0[..4], [255, 255, 255, 255]);
    assert_eq!(f[0].0[4..], [0; 12], "clear where nothing covers");
    // **Stored, the clear ground comes back at the wrap**: with no global
    // table a white pixel on a 2 by 1 screen leaves the other clear, a
    // later frame paints it red, and the first frame again shows it clear.
    // Fails if a record over no frame kept the canvas under it instead of
    // filling it with its ground (the fill in `Shape::apply`): the red
    // would stay.
    let mut g = b"GIF89a\x02\x00\x01\x00\x00\x00\x00".to_vec();
    for (x, colour) in [(0u8, W), (1, R)] {
        g.extend([0x21, 0xf9, 4, 1 << 2, 10, 0, 0, 0]);
        g.extend([0x2c, x, 0, 0, 0, 1, 0, 1, 0, 0x80]);
        g.extend(K);
        g.extend(colour);
        g.push(2);
        g.extend(blocks(&lzw(&[1], 2, Full::Clear)));
    }
    g.push(0x3b);
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2, "the premise: two frames");
    assert_eq!(
        f[0].0,
        [255, 255, 255, 255, 0, 0, 0, 0],
        "the premise: clear"
    );
    assert_eq!(f[1].0, rgba(&[W, R]), "the premise: painted");
    stored_is_decoded(&g, "no global table");
}

/// **Disposal 3, "Restore to previous"**, and the transparency index,
/// which leaves "the corresponding pixel of the display device ... not
/// modified".
#[test]
fn disposal_restores_and_transparency_leaves_the_pixel() {
    let full = Img {
        rect: [0, 0, 2, 2],
        px: vec![3, 3, 3, 3],
        gce: Some((1, 50, None, false)),
        ..Img::default()
    };
    let over = Img {
        rect: [0, 0, 2, 1],
        // Index 0 is transparent: the left pixel keeps the green.
        px: vec![0, 1],
        gce: Some((3, 50, Some(0), false)),
        ..Img::default()
    };
    let last = Img {
        rect: [1, 1, 1, 1],
        px: vec![2],
        gce: Some((0, 50, None, false)),
        ..Img::default()
    };
    let g = gif(2, 2, &[K, W, R, G], 0, None, &[full, over, last]);
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 3);
    assert_eq!(f[0].0, rgba(&[G, G, G, G]));
    assert_eq!(f[1].0, rgba(&[G, W, G, G]), "transparent left, white right");
    assert_eq!(f[2].0, rgba(&[G, G, G, R]), "the white restored to green");
    // **Stored, the same frames**: the white disposal 3 restores comes
    // back as the next frame's changed pixels, the green that was there,
    // not the Background Color (black) 23 e would allow.
    stored_is_decoded(&g, "disposal 3");
    with_stored(&g, |store, id| {
        assert!(store.show_frame(id, FrameNo::of(3)));
        store.compose_frames();
        assert_eq!(canvas(store, id), rgba(&[G, G, G, R]), "restored, stored");
    });
}

/// **A flood of jumps composes once**: making a frame the one to show
/// composes nothing, so a thousand and one jumps between two draws cost
/// the one compose the draw's tick runs, to the last of them, and a tick
/// with nothing moved since composes none. Fails if a jump composed at
/// once, a thousand composes.
#[test]
fn a_flood_of_jumps_composes_once() {
    let full = Img {
        rect: [0, 0, 2, 2],
        px: vec![3, 3, 3, 3],
        gce: Some((1, 50, None, false)),
        ..Img::default()
    };
    let over = Img {
        rect: [0, 0, 2, 1],
        px: vec![1, 1],
        gce: Some((3, 50, None, false)),
        ..Img::default()
    };
    let last = Img {
        rect: [1, 1, 1, 1],
        px: vec![2],
        gce: Some((1, 50, None, false)),
        ..Img::default()
    };
    let g = gif(2, 2, &[K, W, R, G], 0, Some(0), &[full, over, last]);
    let want = frames(&g).unwrap();
    with_stored(&g, |store, id| {
        assert_eq!(store.frames_composed, 0, "the premise");
        for i in 0..1001u32 {
            assert!(store.show_frame(id, FrameNo::of(i % 3 + 1)));
        }
        assert_eq!(store.frames_composed, 0, "a jump composes nothing");
        assert_eq!(store.compose_frames(), 1, "the tick composes once");
        assert_eq!(store.frames_composed, 1);
        assert_eq!(canvas(store, id), want[1000 % 3].0, "the last jump's frame");
        assert_eq!(store.compose_frames(), 0, "nothing moved since");
        assert_eq!(store.frames_composed, 1);
        // **A jump the tick composes is a changed frame**, held or not:
        // `animate` owes the draw whatever it composed. Fails if it
        // answered for the clock's moves alone.
        assert!(store.show_frame(id, FrameNo::of(3)));
        let now = HostTime::of_nanos(1_000_000_000);
        assert_eq!(store.animate(now, ImageMotion::Hold), Animated::Changed);
        assert_eq!(canvas(store, id), want[2].0, "the jump's frame");
        assert_eq!(store.animate(now, ImageMotion::Hold), Animated::Same);
    });
}

/// **A local table, an interlaced image and an image past the screen**:
/// the local table colours its own image alone, Appendix E's rows land in
/// order, and what falls off the logical screen is dropped.
#[test]
fn local_tables_interlace_and_clipping() {
    let px: Vec<u8> = (0..9u8).map(|y| y % 4).collect();
    let tall = Img {
        rect: [0, 0, 1, 9],
        px: px.clone(),
        rows: Rows::Interlaced,
        local: Some(vec![K, W, R, B]),
        gce: Some((1, 10, None, false)),
    };
    // A second interlaced frame, over the column the first drew, so its
    // record is the stored path's and not a whole first frame's.
    let again = Img {
        px: (0..9u8).map(|y| (y + 1) % 4).collect(),
        ..tall.clone()
    };
    let g = gif(2, 9, &[G, G], 0, None, &[tall, again]);
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2, "the premise: two frames");
    // Rows sent in the order 0, 8, 4, 2, 6, 1, 3, 5, 7.
    let order = [0usize, 8, 4, 2, 6, 1, 3, 5, 7];
    let table = [K, W, R, B];
    for (sent, &row) in order.iter().enumerate() {
        let want = table[usize::from(px[sent])];
        assert_eq!(f[0].0[row * 8..row * 8 + 3], want, "row {row}");
        assert_eq!(f[0].0[row * 8 + 4..row * 8 + 7], G, "the right column");
    }
    let off = Img {
        rect: [1, 1, 3, 3],
        px: vec![1; 9],
        gce: Some((1, 10, None, false)),
        ..Img::default()
    };
    let f = frames(&gif(2, 2, &[K, W], 0, None, &[off])).unwrap();
    assert_eq!(f[0].0, rgba(&[K, K, K, W]), "one pixel on the screen");
    // Fails if a record of an interlaced frame were laid in its rows'
    // order as sent, Appendix E's passes, and not as they land.
    stored_is_decoded(&g, "interlaced");
}

/// **"If not 0 ... hundredths of a second to wait"**: an image with delay
/// 0 is composited into the next and never shown alone, and a stream
/// that ends on one ends showing it; a stream whose every delay is 0 is
/// one still picture, every image composited.
#[test]
fn a_zero_delay_composites_and_all_zero_is_a_still() {
    let one = |x, v, delay| Img {
        rect: [x, 0, 1, 1],
        px: vec![v],
        gce: Some((1, delay, None, false)),
        ..Img::default()
    };
    let g = gif(
        3,
        1,
        &[K, W, R, G],
        0,
        Some(0),
        &[one(0, 1, 0), one(1, 2, 40), one(2, 3, 0)],
    );
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2, "the first composited into the second");
    assert_eq!(f[0].0, rgba(&[W, R, K]));
    assert_eq!(f[0].1, 400);
    assert_eq!(f[1].0, rgba(&[W, R, G]), "the stream ends on its last");
    assert_eq!(f[1].1, 0, "gapless");
    stored_is_decoded(&g, "composited");
    let g = gif(
        2,
        1,
        &[K, W, R, G],
        0,
        Some(0),
        &[one(0, 1, 0), one(1, 2, 0)],
    );
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 1, "no wait anywhere");
    assert_eq!(f[0].0, rgba(&[W, R]), "both composited");
    assert_eq!(f[0].1, 0, "gapless");
    // A still picture with no Graphic Control Extension.
    let still = Img {
        rect: [0, 0, 1, 1],
        px: vec![1],
        ..Img::default()
    };
    assert_eq!(
        frames(&gif(1, 1, &[K, W], 0, None, &[still]))
            .unwrap()
            .len(),
        1
    );
}

/// **The loop count**: NETSCAPE2.0's 0 for ever, `n` plays `n + 1`
/// times, and with no such block the stream plays once.
#[test]
fn the_loop_count_is_netscapes() {
    let im = Img {
        rect: [0, 0, 1, 1],
        px: vec![1],
        gce: Some((0, 10, None, false)),
        ..Img::default()
    };
    let two = [im.clone(), im];
    for (loops, plays) in [(None, 1), (Some(0), 0), (Some(3), 4), (Some(65535), 65536)] {
        let g = gif(1, 1, &[K, W], 0, loops, &two);
        assert_eq!(Gif::parse(&g).unwrap().plays(), plays, "{loops:?}");
    }
}

/// **The User Input Flag** marks its frame as waiting for input, delay or
/// none: with none it is shown, not composited away.
#[test]
fn a_frame_with_the_user_input_flag_waits() {
    let one = |x, v, gce| Img {
        rect: [x, 0, 1, 1],
        px: vec![v],
        gce: Some(gce),
        ..Img::default()
    };
    let g = gif(
        2,
        1,
        &[K, W, R, G],
        0,
        None,
        &[
            one(0, 1, (1, 0, None, true)),
            one(1, 2, (1, 30, None, false)),
        ],
    );
    let f = frames(&g).unwrap();
    assert_eq!(f.len(), 2);
    assert_eq!((f[0].1, f[0].2), (0, GifWait::Input));
    assert_eq!((f[1].1, f[1].2), (300, GifWait::Delay));
}

/// **A Plain Text Extension** (25): its grid `[left, top, width, height]`,
/// its character cell `[width, height]`, its foreground and background
/// indices, and its text in sub-blocks.
fn text(grid: [u16; 4], cell: [u8; 2], fg: u8, bg: u8, data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x21, 0x01, 12];
    for v in grid {
        out.extend(v.to_le_bytes());
    }
    out.extend([cell[0], cell[1], fg, bg]);
    out.extend(blocks(data));
    out
}

/// A Graphic Control Extension: its disposal, delay and transparency index.
fn control(disposal: u8, delay: u16, t: Option<u8>) -> Vec<u8> {
    let mut out = vec![0x21, 0xf9, 4, disposal << 2 | u8::from(t.is_some())];
    out.extend(delay.to_le_bytes());
    out.extend([t.unwrap_or(0), 0]);
    out
}

/// `g` with `more` written before its trailer.
fn and_then(mut g: Vec<u8>, more: &[Vec<u8>]) -> Vec<u8> {
    g.pop();
    for b in more {
        g.extend(b);
    }
    g.push(0x3b);
    g
}

/// The bytes of `im` as [`gif`] writes it over `gct`: its control and its
/// image, no header and no trailer.
fn image_bytes(gct: &[[u8; 3]], im: Img) -> Vec<u8> {
    let g = gif(1, 1, gct, 0, None, &[im]);
    g[13 + palette(gct).0.len()..g.len() - 1].to_vec()
}

/// **The stub font**: the cell's left column for every character but the
/// space, and nothing for the space. Never the real face.
fn stub(c: char, _: ImagePx, h: ImagePx, set: &mut dyn FnMut(ImagePx, ImagePx)) {
    if c != ' ' {
        for y in 0..h.raw() {
            set(ImagePx::of(0), ImagePx::of(y));
        }
    }
}

thread_local! {
    static ASKED: RefCell<Vec<char>> = const { RefCell::new(Vec::new()) };
}

/// [`stub`], keeping each character it is asked to draw.
fn keeping(c: char, w: ImagePx, h: ImagePx, set: &mut dyn FnMut(ImagePx, ImagePx)) {
    ASKED.with(|a| a.borrow_mut().push(c));
    stub(c, w, h, set);
}

/// The characters [`keeping`] was asked to draw since the last call.
fn asked() -> Vec<char> {
    ASKED.with(RefCell::take)
}

/// **A Plain Text Extension is drawn through the lent font** (25): each
/// whole cell of its grid in the background index, the pixels the font
/// sets in the foreground index, both of the global table, the text from
/// the upper left cell, left to right, and nothing counted. Fails if no
/// text is drawn, or if the block is counted.
#[test]
fn a_plain_text_extension_is_drawn_through_the_lent_font() {
    // Foreground W, background R: two 4 by 2 cells across an 8 by 2 screen.
    let g = and_then(
        gif(8, 2, &[K, W, R, G], 0, None, &[]),
        &[text([0, 0, 8, 2], [4, 2], 1, 2, b"AB")],
    );
    let f = frames_with(&g, Some(stub)).unwrap();
    assert_eq!(f.len(), 1);
    let row = [W, R, R, R, W, R, R, R];
    assert_eq!(f[0].0, rgba(&[row, row].concat()));
    let parsed = Gif::parse(&g).unwrap().with_font(Some(stub));
    assert_eq!(parsed.texts_not_drawn(), 0);
}

/// **A byte below 0x20 or above 0xF7 is drawn as a space** (25 e), and so
/// are DEL and the C1 controls, 0x7F to 0x9F; 0xA0 to 0xF7 are ISO
/// 8859-1's characters. The cells of 0x1F and 0xF8 are the cell of 0x20.
/// Fails if a byte out of range reaches the font as anything but a space.
#[test]
fn a_byte_out_of_range_is_drawn_as_a_space() {
    let data = [0x1f, 0x20, 0xf8, b'A', 0x7f, 0x9f, 0xa0, 0xe9, 0xf7];
    let g = and_then(
        gif(9, 1, &[K, W, R, G], 0, None, &[]),
        &[text([0, 0, 9, 1], [1, 1], 1, 2, &data)],
    );
    asked();
    let f = frames_with(&g, Some(keeping)).unwrap();
    assert_eq!(
        asked(),
        [' ', ' ', ' ', 'A', ' ', ' ', '\u{a0}', '\u{e9}', '\u{f7}']
    );
    assert_eq!(f[0].0, rgba(&[R, R, R, W, R, R, W, W, W]));
}

/// **"fractional cells must be discarded"**: a grid 20 wide of cells 8
/// wide is two cells, and columns 16 to 19 keep the canvas. Fails if a
/// third cell is drawn there, whole or cut.
#[test]
fn a_fractional_cell_is_not_drawn() {
    let g = and_then(
        gif(24, 1, &[K, W, R, G], 0, None, &[]),
        &[text([0, 0, 20, 1], [8, 1], 1, 2, b"ABC")],
    );
    let f = frames_with(&g, Some(stub)).unwrap();
    let mut want = [K; 24];
    want[..16].fill(R);
    want[0] = W;
    want[8] = W;
    assert_eq!(f[0].0, rgba(&want));
}

/// **The text ends where "the character grid is filled"**: a grid of two
/// rows of two cells takes four bytes of five, and the row below the grid
/// keeps the canvas. Fails if a byte past the full grid is drawn.
#[test]
fn text_past_the_full_grid_is_not_drawn() {
    let g = and_then(
        gif(8, 3, &[K, W, R, G], 0, None, &[]),
        &[text([0, 0, 8, 2], [4, 1], 1, 2, b"ABCDE")],
    );
    asked();
    let f = frames_with(&g, Some(keeping)).unwrap();
    assert_eq!(asked(), ['A', 'B', 'C', 'D']);
    let row = [W, R, R, R, W, R, R, R];
    assert_eq!(f[0].0, rgba(&[row, row, [K; 8]].concat()));
}

/// **What the font sets outside its cell is dropped**: the font is the
/// embedder's, and may set a pixel past the cell it was given.
#[test]
fn a_font_drawing_past_its_cell_is_clipped() {
    fn spill(_: char, w: ImagePx, h: ImagePx, set: &mut dyn FnMut(ImagePx, ImagePx)) {
        set(w, ImagePx::of(0));
        set(ImagePx::of(0), h);
        set(ImagePx::of(u16::MAX), ImagePx::of(u16::MAX));
    }
    let g = and_then(
        gif(4, 2, &[K, W, R, G], 0, None, &[]),
        &[text([0, 0, 2, 1], [2, 1], 1, 2, b"A")],
    );
    let f = frames_with(&g, Some(spill)).unwrap();
    assert_eq!(f[0].0, rgba(&[R, R, K, K, K, K, K, K]));
}

/// **The transparency index acts on the text** (23 d, viii), as it does
/// on an image: as the background index, the image under the grid shows
/// between the glyph's pixels, and as the foreground index, under the
/// glyph. Fails if the text is drawn over the canvas whatever the index.
#[test]
fn a_transparent_index_keeps_the_image_under_the_text() {
    let under = Img {
        rect: [0, 0, 4, 2],
        px: vec![3; 8],
        ..Img::default()
    };
    for (t, row) in [(2, [W, G, G, G]), (1, [G, R, R, R])] {
        let g = and_then(
            gif(4, 2, &[K, W, R, G], 0, None, std::slice::from_ref(&under)),
            &[
                control(0, 0, Some(t)),
                text([0, 0, 4, 2], [4, 2], 1, 2, b"A"),
            ],
        );
        let f = frames_with(&g, Some(stub)).unwrap();
        assert_eq!(f.len(), 1, "both composited");
        assert_eq!(f[0].0, rgba(&[row, row].concat()), "transparency index {t}");
    }
}

/// **The text's Graphic Control Extension delays and disposes it**: it is
/// a frame of its own, and disposal 2 restores the background under its
/// grid before the image after it. Fails if the text stays on the canvas,
/// or is composited into the next frame.
#[test]
fn a_texts_disposal_restores_the_background() {
    let corner = Img {
        rect: [3, 1, 1, 1],
        px: vec![3],
        ..Img::default()
    };
    let gct = [K, W, R, G];
    let g = and_then(
        gif(4, 2, &gct, 0, None, &[]),
        &[
            control(2, 10, None),
            text([0, 0, 4, 2], [4, 2], 1, 2, b"A"),
            image_bytes(&gct, corner),
        ],
    );
    let f = frames_with(&g, Some(stub)).unwrap();
    assert_eq!(f.len(), 2, "the text's frame and the image's");
    assert_eq!(f[0].0, rgba(&[W, R, R, R, W, R, R, R]));
    assert_eq!(f[0].1, 100);
    assert_eq!(f[1].0, rgba(&[K, K, K, K, K, K, K, G]));
}

/// **A Plain Text Extension not drawn is counted**: with no font lent, with
/// no global table, which "This block requires" (25), and with a header
/// short of its twelve bytes, whose Graphic Control Extension is still
/// taken, so the image after it is a frame of its own. Fails if a block
/// not drawn goes uncounted, or one is drawn with no font.
#[test]
fn a_plain_text_extension_not_drawn_is_counted() {
    let g = and_then(
        gif(8, 2, &[K, W, R, G], 0, None, &[]),
        &[text([0, 0, 8, 2], [4, 2], 1, 2, b"AB")],
    );
    assert_eq!(Gif::parse(&g).unwrap().texts_not_drawn(), 1, "no font");
    assert_eq!(frames(&g).unwrap()[0].0, rgba(&[K; 16]), "no font, no text");

    let mut bare = b"GIF89a\x08\x00\x02\x00\x00\x00\x00".to_vec();
    bare.extend(text([0, 0, 8, 2], [4, 2], 1, 2, b"AB"));
    bare.push(0x3b);
    let parsed = Gif::parse(&bare).unwrap().with_font(Some(stub));
    assert_eq!(parsed.texts_not_drawn(), 1, "no global table");
    assert_eq!(frames_with(&bare, Some(stub)).unwrap()[0].0, vec![0; 64]);

    let mut short = gif(2, 1, &[K, W], 0, None, &[]);
    short.pop();
    short.extend([0x21, 0xf9, 4, 4, 20, 0, 0, 0]);
    short.extend([0x21, 0x01, 8, 0, 0, 0, 0, 2, 0, 1, 0]);
    short.extend(blocks(b"hi"));
    short.extend(b"\x2c\x01\x00\x00\x00\x01\x00\x01\x00\x00\x02");
    short.extend(blocks(&lzw(&[1], 2, Full::Clear)));
    short.push(0x3b);
    let parsed = Gif::parse(&short).unwrap().with_font(Some(stub));
    assert_eq!(parsed.texts_not_drawn(), 1, "a short header");
    let f = frames_with(&short, Some(stub)).unwrap();
    assert_eq!(f.len(), 2, "the text's frame and the image's");
    assert_eq!(f[0].0, rgba(&[K, K]), "no text drawn");
    assert_eq!(f[1].0, rgba(&[K, W]));
}

/// **A stream cut short shows what arrived**: an image whose data stops
/// keeps the canvas where its pixels never came.
#[test]
fn a_stream_cut_short_shows_what_arrived() {
    let px: Vec<u8> = (0..64u8).map(|i| 1 + i % 2).collect();
    let im = Img {
        rect: [0, 0, 8, 8],
        px,
        ..Img::default()
    };
    let g = gif(8, 8, &[K, W, R], 0, None, &[im]);
    let whole = frames(&g).unwrap();
    let cut = frames(&g[..g.len() - 8]).unwrap();
    assert_eq!(cut.len(), 1);
    assert_eq!(cut[0].0[..32], whole[0].0[..32], "the first row arrived");
    assert_ne!(cut[0].0, whole[0].0);
    assert_eq!(
        cut[0].0[252..],
        [0, 0, 0, 255],
        "the last pixel is the background"
    );
}

/// The ways a stream is harmed, each a class the property names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Harm {
    /// An image's LZW minimum code size set to anything.
    CodeSize,
    /// An image's data replaced by codes that fill the table with no Clear.
    NoClear,
    /// An image's data starting with a code that is no Clear.
    FirstNotClear,
    /// An image moved past the logical screen.
    OffScreen,
    /// A sub-block's length made longer than what follows.
    SubBlock,
    /// The logical screen at 65535 by 65535.
    Size,
    /// The stream cut at a block.
    Cut,
    /// A byte anywhere set to anything.
    Byte,
}

const HARMS: [Harm; 8] = [
    Harm::CodeSize,
    Harm::NoClear,
    Harm::FirstNotClear,
    Harm::OffScreen,
    Harm::SubBlock,
    Harm::Size,
    Harm::Cut,
    Harm::Byte,
];

/// The positions of each image descriptor in `g`, found by walking.
fn images(g: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut pos = 13
        + match g.get(10) {
            Some(f) if f & 0x80 != 0 => 3 * (2usize << (f & 7)),
            _ => 0,
        };
    while let Some(&b) = g.get(pos) {
        match b {
            0x21 => {
                pos += 2;
                while let Some(&n) = g.get(pos) {
                    pos += 1 + usize::from(n);
                    if n == 0 {
                        break;
                    }
                }
            }
            0x2c => {
                out.push(pos);
                let f = g.get(pos + 9).copied().unwrap_or(0);
                pos += 10
                    + match f & 0x80 {
                        0 => 0,
                        _ => 3 * (2usize << (f & 7)),
                    };
                pos += 1;
                while let Some(&n) = g.get(pos) {
                    pos += 1 + usize::from(n);
                    if n == 0 {
                        break;
                    }
                }
            }
            _ => break,
        }
    }
    out
}

/// Where image `i`'s minimum code size is.
fn data_of(g: &[u8], i: usize) -> usize {
    let f = g.get(i + 9).copied().unwrap_or(0);
    i + 10
        + match f & 0x80 {
            0 => 0,
            _ => 3 * (2usize << (f & 7)),
        }
}

fn harmed(g: &[u8], harm: Harm, pick: usize, v: u8) -> (Vec<u8>, bool) {
    let mut g = g.to_vec();
    let ims = images(&g);
    let at = (!ims.is_empty()).then(|| ims[pick % ims.len()]);
    let applied = match harm {
        Harm::CodeSize => at.map(|i| {
            let d = data_of(&g, i);
            if d < g.len() {
                g[d] = v;
            }
        }),
        Harm::NoClear | Harm::FirstNotClear => at.map(|i| {
            let d = data_of(&g, i);
            if d + 1 < g.len() {
                // Codes 2, 3, ... at a width that grows to twelve: a table
                // filled past 4096 with no Clear, or one begun without it.
                let m = 2u8;
                let mut codes: Vec<u8> = Vec::new();
                let (mut acc, mut n, mut width, mut next) = (0u32, 0u32, 3u32, 6u32);
                let start = u32::from(harm == Harm::FirstNotClear);
                for k in 0..5000u32 {
                    let c = match (k, start) {
                        (0, 0) => 4,
                        _ => (k + u32::from(v)) % next.min(4096),
                    };
                    acc |= c << n;
                    n += width;
                    while n >= 8 {
                        codes.push(acc.to_le_bytes()[0]);
                        acc >>= 8;
                        n -= 8;
                    }
                    if k > 0 && next < 4096 {
                        next += 1;
                        if next == 1 << width && width < 12 {
                            width += 1;
                        }
                    }
                }
                let tail = g.split_off(d);
                let rest = tail
                    .iter()
                    .position(|&b| b == 0x3b)
                    .map_or(&[][..], |p| &tail[p..]);
                g.push(m);
                g.extend(blocks(&codes));
                g.extend_from_slice(rest);
            }
        }),
        Harm::OffScreen => at.map(|i| {
            if i + 4 < g.len() {
                g[i + 1] = v;
                g[i + 2] = 0xff;
                g[i + 3] = v.rotate_left(1);
                g[i + 4] = 0xfe;
            }
        }),
        Harm::SubBlock => at.map(|i| {
            let d = data_of(&g, i);
            if d + 1 < g.len() {
                g[d + 1] = v | 0x80;
            }
        }),
        Harm::Size => (g.len() > 10).then(|| {
            for b in &mut g[6..10] {
                *b = 0xff;
            }
        }),
        Harm::Cut => at.map(|i| g.truncate(i + usize::from(v % 16))),
        Harm::Byte => (!g.is_empty()).then(|| {
            let i = pick % g.len();
            g[i] = v;
        }),
    };
    (g, applied.is_some())
}

/// What any decode may cost: the scratch it asks for is the screen's, and
/// the decode returns, whatever it returns.
fn survives(g: &[u8]) {
    let Ok(gif) = Gif::parse(g) else {
        return;
    };
    let unfonted = gif.texts_not_drawn();
    let gif = gif.with_font(Some(stub));
    assert!(
        gif.texts_not_drawn() <= unfonted,
        "a font draws, never counts more"
    );
    let need = gif.scratch_len();
    if need > 1 << 26 {
        return;
    }
    let mut scratch = vec![0u8; need];
    let mut n = 0u32;
    let _ = gif.decode(&mut scratch, |f| {
        assert_eq!(f.rgba.len(), need / 2, "a frame is the screen");
        n += 1;
        true
    });
    assert!(n <= gif.frames(), "no more frames than counted");
    let mut short = vec![0u8; need - 1];
    assert_eq!(gif.decode(&mut short, |_| true), Err(GifError::Buffer));
}

fn seeds() -> Vec<Vec<u8>> {
    let one = |x, v, gce| Img {
        rect: [x, 0, 2, 2],
        px: vec![v; 4],
        gce: Some(gce),
        ..Img::default()
    };
    vec![
        include_bytes!("gif/scene64i.gif").to_vec(),
        include_bytes!("gif/scene2.gif").to_vec(),
        gif(
            4,
            2,
            &[K, W, R, G],
            1,
            Some(2),
            &[
                one(0, 1, (2, 10, Some(0), false)),
                one(1, 2, (3, 0, None, true)),
                one(2, 3, (1, 20, Some(3), false)),
            ],
        ),
        and_then(
            gif(
                8,
                2,
                &[K, W, R, G],
                0,
                None,
                &[one(0, 1, (1, 10, None, false))],
            ),
            &[
                control(2, 10, Some(2)),
                text([1, 0, 7, 2], [3, 2], 1, 2, b"hello"),
            ],
        ),
    ]
}

proptest! {
    #[test]
    fn a_malformed_stream_never_panics(
        seed in 0..4usize,
        harms in proptest::collection::vec((0..HARMS.len(), any::<usize>(), any::<u8>()), 1..4),
    ) {
        let mut g = seeds()[seed].clone();
        for (h, pick, v) in harms {
            g = harmed(&g, HARMS[h], pick, v).0;
        }
        survives(&g);
    }
}

/// **The positive control**: every class applies to some seed and makes a
/// stream that differs from it.
#[test]
fn every_harm_class_is_generated() {
    for harm in HARMS {
        let fired = seeds().iter().any(|s| {
            (0..8).any(|pick| {
                let (g, applied) = harmed(s, harm, pick, 0x5a);
                applied && g != *s
            })
        });
        assert!(fired, "{harm:?} never changes a stream");
    }
}
