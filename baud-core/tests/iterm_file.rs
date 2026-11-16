//! iTerm2's inline images (`[[source: iterm2-images]]`): `File=args:base64`
//! and `MultipartFile`, `FilePart`, `FileEnd`, at every read size, each
//! picture's pixels, place and cursor against what iTerm2's arithmetic gives
//! (`[[source: iterm2-964a75d-inline-image-helper]]`). The PNGs are written
//! by the `png` crate, never by the decoder under test.

use baud_core::*;
use baud_domain::{
    Animated, HostTime, ImageBudget, ImageDraw, ImageMotion, ImagePx, ItermKey, KittyCounts,
    Messages, Pixel,
};

const WS: WinSize = WinSize {
    ws_row: 24,
    ws_col: 80,
};

/// The read sizes: a byte at a time, a prime that splits every key and
/// quantum, one past a 4096-byte read, and whole.
const SIZES: [usize; 4] = [1, 7, 4097, 1 << 20];

const ONE: Messages = Messages::of(1);

/// A terminal with `pixels` of arena, a 10x20 cell and a white foreground.
fn with<R>(pixels: usize, f: impl FnOnce(&mut Term<'_>) -> R) -> R {
    let mut owned = Owned::new(&WS).with_images(ImageBudget::of(pixels));
    let mut t = Term::new(owned.storage(), &WS);
    t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
    feed(&mut t, b"\x1b]10;rgb:ff/ff/ff\x07", 4096);
    f(&mut t)
}

fn feed(t: &mut Term<'_>, bytes: &[u8], size: usize) {
    for read in bytes.chunks(size) {
        t.write_at(BytePos::ZERO, read);
        t.feed(BytePos::ZERO, ReadLen::from_index(read.len()));
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for q in data.chunks(3) {
        let mut b = [0u8; 3];
        b[..q.len()].copy_from_slice(q);
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        for k in 0..4 {
            match k <= q.len() {
                true => out.push(B64[usize::try_from((n >> (18 - 6 * k)) & 63).unwrap()]),
                false => out.push(b'='),
            }
        }
    }
    out
}

/// An RGBA PNG of `w` by `h` with noisy pixels, and its pixels as the store
/// keeps them.
fn picture(w: u32, h: u32, seed: u32) -> (Vec<u8>, Vec<u32>) {
    let mut x = seed.max(1);
    let data: Vec<u8> = (0..w * h * 4)
        .map(|i| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            // Every fourth byte, the alpha, opaque, so the picture covers.
            match i % 4 {
                3 => 0xff,
                _ => x.to_le_bytes()[0],
            }
        })
        .collect();
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        let mut wr = e.write_header().unwrap();
        wr.write_image_data(&data).unwrap();
    }
    let c = Chan::from_u8;
    let px = data
        .chunks(4)
        .map(|p| Pixel::straight(c(p[3]), c(p[0]), c(p[1]), c(p[2])).bits())
        .collect();
    (out, px)
}

fn file(args: &str, data: &[u8]) -> Vec<u8> {
    let mut v = format!("\x1b]1337;File={args}:").into_bytes();
    v.extend(b64(data));
    v.push(0x07);
    v
}

/// The one placement: its column, its image's size and pixels.
fn placed(t: &Term<'_>) -> (usize, u16, u16, Vec<u32>) {
    let imgs = t.images();
    let mut ps = imgs.placements();
    let (_, p) = ps.next().expect("a placement");
    assert!(ps.next().is_none(), "one placement");
    let v = imgs.image(p.image).expect("shown");
    (
        p.col.index(),
        v.width.raw(),
        v.height.raw(),
        v.pixels.iter().map(|p| p.bits()).collect(),
    )
}

/// **"auto: The image's inherent size"**: a PNG placed at the cursor one
/// pixel a device pixel, its cells `ceil(30 / 10)` by `ceil(40 / 20)`, the
/// cursor on its last row right of it, at every read size -- a picture
/// whose base64 passes 4096 bytes, so the routed body crosses reads.
#[test]
fn an_inline_png_is_placed_at_its_own_size() {
    let (png, px) = picture(30, 40, 7);
    assert!(b64(&png).len() > 4096, "the body crosses a read");
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, b"\x1b[3;5H", size);
            feed(t, &file("inline=1", &png), size);
            let (col, w, h, got) = placed(t);
            assert_eq!((col, w, h), (4, 30, 40), "reads of {size}");
            assert!(got == px, "the pixels, reads of {size}");
            assert_eq!(
                t.cursor(),
                (Row::from_index(3), Col::at_index(7)),
                "reads of {size}"
            );
            assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
        });
    }
}

/// The IJG's baseline test image and its reference decode
/// (`[[source: ijg-jpeg-6b]]`), as `djpeg -dct int` writes it.
const TESTORIG: &[u8] = include_bytes!("../../baud-domain/tests/ijg/testorig.jpg");
const TESTIMG: &[u8] = include_bytes!("../../baud-domain/tests/ijg/testimg.ppm");

/// The reference decode's pixels as the store keeps them, opaque.
fn testimg() -> Vec<u32> {
    // "P6\n227 149\n255\n", fifteen bytes.
    assert_eq!(&TESTIMG[..15], b"P6\n227 149\n255\n");
    TESTIMG[15..]
        .chunks(3)
        .map(|p| u32::from_be_bytes([0xff, p[0], p[1], p[2]]))
        .collect()
}

/// **A JPEG draws, not the placeholder**: `imgcat`'s JPEG is "Any image
/// format that macOS supports", placed at its own size at the cursor, its
/// pixels the reference decode's, the cursor right of it on its last row,
/// at every read size; with an Exif orientation of 6 the picture is shown
/// turned a quarter clockwise, its sides swapped.
#[test]
fn an_inline_jpeg_is_placed_as_it_decodes() {
    let want = testimg();
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, b"\x1b[3;5H", size);
            feed(t, &file("inline=1", TESTORIG), size);
            let (col, w, h, got) = placed(t);
            assert_eq!((col, w, h), (4, 227, 149), "reads of {size}");
            assert!(got == want, "the pixels, reads of {size}");
            // 149 pixels are eight rows of 20; 227 are 23 columns of 10.
            assert_eq!(
                t.cursor(),
                (Row::from_index(9), Col::at_index(27)),
                "reads of {size}"
            );
            assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
        });
    }
    // Exif's tag 274 at 6, big-endian, after the JFIF APP0's 18 bytes.
    let mut turned = TESTORIG[..20].to_vec();
    let tiff = b"MM\x00\x2a\x00\x00\x00\x08\x00\x01\x01\x12\x00\x03\x00\x00\x00\x01\x00\x06\x00\x00\x00\x00\x00\x00";
    let len = u16::try_from(2 + 6 + tiff.len()).unwrap();
    turned.extend([0xff, 0xe1]);
    turned.extend(len.to_be_bytes());
    turned.extend(b"Exif\0\0");
    turned.extend(tiff);
    turned.extend_from_slice(&TESTORIG[20..]);
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &turned), 4097);
        let (_, w, h, got) = placed(t);
        assert_eq!((w, h), (149, 227), "the sides swapped");
        // Shown (x, y) is coded (y, 148 - x).
        for y in 0..227 {
            for x in 0..149 {
                assert_eq!(got[y * 149 + x], want[(148 - x) * 227 + y], "({x}, {y})");
            }
        }
    });
}

/// **A JPEG this decoder does not implement draws the placeholder**, and is
/// counted: a differential frame of the hierarchical process, `SOF5`.
#[test]
fn a_jpeg_process_not_built_draws_the_placeholder() {
    let sof = TESTORIG.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
    let mut f = TESTORIG.to_vec();
    f[sof + 1] = 0xc5;
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &f), 4097);
        let (_, w, h, _) = placed(t);
        assert_eq!((w, h), (35, 36), "the placeholder");
        assert_eq!(t.iterm_counts().of(ItermKey::File), ONE);
    });
}

/// **A 2x2 bitmap**: the file header, a 40-byte BITMAPINFOHEADER at 24
/// bits, and its two rows bottom-up, each padded to four bytes -- blue and
/// green, then red and white (`[[source: ms-bitmapinfoheader]]`).
fn bmp() -> Vec<u8> {
    [
        // The file header: the file's 70 bytes, and the bits at 54.
        &b"BM\x46\0\0\0\0\0\0\0\x36\0\0\0"[..],
        // 40 bytes, 2 by 2, one plane, 24 bits, BI_RGB, 16 bytes of image.
        b"\x28\0\0\0\x02\0\0\0\x02\0\0\0\x01\0\x18\0\0\0\0\0\x10\0\0\0",
        // The resolutions, biClrUsed and biClrImportant.
        b"\x13\x0b\0\0\x13\x0b\0\0\0\0\0\0\0\0\0\0",
        // Blue and green, then red and white.
        b"\xff\0\0\0\xff\0\0\0",
        b"\0\0\xff\xff\xff\xff\0\0",
    ]
    .concat()
}

/// **A bitmap a program sends is placed as it decodes**, at every read size:
/// its own size, its rows turned top-down, the cursor right of it; and one
/// whose header is cut short draws the placeholder and is counted.
#[test]
fn an_inline_bmp_is_placed_as_it_decodes() {
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, b"\x1b[3;5H", size);
            feed(t, &file("inline=1", &bmp()), size);
            let (col, w, h, got) = placed(t);
            assert_eq!((col, w, h), (4, 2, 2), "reads of {size}");
            assert_eq!(
                got,
                [0xFFFF_0000, 0xFFFF_FFFF, 0xFF00_00FF, 0xFF00_FF00],
                "reads of {size}"
            );
            assert_eq!(
                t.cursor(),
                (Row::from_index(2), Col::at_index(5)),
                "reads of {size}"
            );
            assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
        });
    }
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &bmp()[..30]), 4097);
        let (_, w, h, _) = placed(t);
        assert_eq!((w, h), (35, 36), "the placeholder");
        assert_eq!(t.iterm_counts().of(ItermKey::File), ONE);
    });
}

/// One graphic of a GIF this file writes: its rectangle, indices and
/// Graphic Control Extension (disposal, delay, user input).
struct Graphic {
    rect: [u16; 4],
    px: Vec<u8>,
    gce: Option<(u8, u16, bool)>,
}

/// **A GIF89a stream** over the colours `K`, `W`, `R`, `G`: a `w` by `h`
/// screen, background index 0, a NETSCAPE2.0 block with `loops` if given,
/// and each graphic's indices coded as Appendix F allows with a Clear code
/// before every one, so the code width never grows.
fn gif(w: u16, h: u16, loops: Option<u16>, graphics: &[Graphic]) -> Vec<u8> {
    gif_tables(w, h, loops, graphics, Tables::Global)
}

/// Where a stream this file writes keeps its colours.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tables {
    /// One global table, its Background Color index 0, opaque black.
    Global,
    /// None global, so where no image covers the screen it is clear, and
    /// the same four colours as each image's local table.
    Local,
}

/// [`gif`], its colours kept as `tables` says.
fn gif_tables(w: u16, h: u16, loops: Option<u16>, graphics: &[Graphic], tables: Tables) -> Vec<u8> {
    const TABLE: [u8; 12] = [0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 255, 0];
    let mut out = b"GIF89a".to_vec();
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    match tables {
        Tables::Global => {
            out.extend([0xf1, 0, 0]);
            out.extend(TABLE);
        }
        Tables::Local => out.extend([0, 0, 0]),
    }
    if let Some(n) = loops {
        out.extend(b"\x21\xff\x0bNETSCAPE2.0\x03\x01");
        out.extend(n.to_le_bytes());
        out.push(0);
    }
    for g in graphics {
        if let Some((disposal, delay, input)) = g.gce {
            out.extend([0x21, 0xf9, 4, disposal << 2 | u8::from(input) << 1]);
            out.extend(delay.to_le_bytes());
            out.extend([0, 0]);
        }
        out.push(0x2c);
        for v in g.rect {
            out.extend(v.to_le_bytes());
        }
        match tables {
            Tables::Global => out.push(0),
            Tables::Local => {
                out.push(0x81);
                out.extend(TABLE);
            }
        }
        out.push(2);
        // Clear (4) then the index, three bits each, packed right to left.
        let mut bits: Vec<bool> = Vec::new();
        let mut code = |c: u8| (0..3).for_each(|k| bits.push(c >> k & 1 == 1));
        for &i in &g.px {
            code(4);
            code(i);
        }
        code(5);
        let bytes: Vec<u8> = bits
            .chunks(8)
            .map(|c| c.iter().rev().fold(0u8, |b, &x| b << 1 | u8::from(x)))
            .collect();
        for c in bytes.chunks(255) {
            out.push(u8::try_from(c.len()).unwrap());
            out.extend_from_slice(c);
        }
        out.push(0);
    }
    out.push(0x3b);
    out
}

/// The opaque word of each colour index.
const GIF_COLOURS: [u32; 4] = [0xff00_0000, 0xffff_ffff, 0xffff_0000, 0xff00_ff00];

/// A whole-screen graphic of one colour and its delay.
fn flat(w: u16, h: u16, i: u8, delay: u16) -> Graphic {
    let n = usize::from(w) * usize::from(h);
    Graphic {
        rect: [0, 0, w, h],
        px: vec![i; n],
        gce: Some((1, delay, false)),
    }
}

fn gif_at(ms: u64) -> HostTime {
    HostTime::of_nanos(1_000_000_000 + ms * 1_000_000)
}

/// **An animated GIF plays**: iTerm2 plays a GIF's frames
/// (`[[source: iterm2-964a75d-image-with-data]]`), each for its delay,
/// "the number of hundredths (1/100) of a second to wait"; NETSCAPE2.0's
/// count of 0 loops for ever. Its pixels change on the clock, and held
/// (Reduce Motion) they do not.
#[test]
fn an_inline_gif_plays_its_frames() {
    let g = gif(
        20,
        20,
        Some(0),
        &[
            flat(20, 20, 1, 10),
            flat(20, 20, 2, 20),
            flat(20, 20, 3, 30),
        ],
    );
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &file("inline=1", &g), size);
            let shown = |t: &Term<'_>| placed(t).3[0];
            assert_eq!(shown(t), GIF_COLOURS[1], "the first frame, reads of {size}");
            assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
            assert_eq!(
                t.animate_images(gif_at(0), ImageMotion::Play),
                Animated::Same
            );
            for (ms, frame) in [(100, 2), (300, 3), (600, 1), (700, 2)] {
                assert_eq!(
                    t.animate_images(gif_at(ms), ImageMotion::Play),
                    Animated::Changed,
                    "{ms} ms"
                );
                assert_eq!(shown(t), GIF_COLOURS[frame], "{ms} ms, reads of {size}");
            }
            assert_eq!(
                t.animate_images(gif_at(5000), ImageMotion::Hold),
                Animated::Same
            );
            assert_eq!(shown(t), GIF_COLOURS[2], "held");
        });
    }
}

/// **With no NETSCAPE2.0 block the stream is processed once** and stays
/// on its last frame.
#[test]
fn a_gif_with_no_loop_count_plays_once() {
    let g = gif(10, 10, None, &[flat(10, 10, 1, 10), flat(10, 10, 2, 10)]);
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &g), 4097);
        t.animate_images(gif_at(0), ImageMotion::Play);
        assert_eq!(
            t.animate_images(gif_at(100), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(placed(t).3[0], GIF_COLOURS[2]);
        assert_eq!(
            t.animate_images(gif_at(200), ImageMotion::Play),
            Animated::Same
        );
        assert_eq!(
            t.animate_images(gif_at(900), ImageMotion::Play),
            Animated::Same
        );
        assert_eq!(placed(t).3[0], GIF_COLOURS[2], "the last frame stays");
    });
}

/// **A stream that ends on a graphic with a delay of 0 ends showing it**:
/// "Process each graphic in the Data Stream in sequence" (`[[source:
/// gif89a]]` 8), so the last graphic, composited over the one before,
/// is what stays once the last play ends. Played once with no NETSCAPE2.0
/// block, and twice with a count of 1, back on the first frame between.
#[test]
fn a_gif_that_ends_on_a_zero_delay_ends_showing_it() {
    for (loops, plays) in [(None, 1u64), (Some(1), 2)] {
        let g = gif(
            10,
            10,
            loops,
            &[flat(10, 10, 1, 10), flat(10, 10, 2, 10), flat(10, 10, 3, 0)],
        );
        with(1 << 16, |t| {
            feed(t, &file("inline=1", &g), 4097);
            t.animate_images(gif_at(0), ImageMotion::Play);
            let mut ms = 0;
            for play in 1..=plays {
                ms += 100;
                t.animate_images(gif_at(ms), ImageMotion::Play);
                assert_eq!(placed(t).3[0], GIF_COLOURS[2], "{loops:?}, play {play}");
                ms += 100;
                assert_eq!(
                    t.animate_images(gif_at(ms), ImageMotion::Play),
                    Animated::Changed,
                    "{loops:?}, play {play}"
                );
                let want = match play == plays {
                    true => GIF_COLOURS[3],
                    false => GIF_COLOURS[1],
                };
                assert_eq!(placed(t).3[0], want, "{loops:?}, play {play} ends");
            }
            assert_eq!(t.next_image_frame(gif_at(ms)), None, "nothing due");
            assert_eq!(
                t.animate_images(gif_at(ms + 5000), ImageMotion::Play),
                Animated::Same
            );
            assert_eq!(
                placed(t).3[0],
                GIF_COLOURS[3],
                "{loops:?}: the last graphic stays"
            );
        });
    }
}

/// **"If the flag is set, processing will continue when user input is
/// entered"** (`[[source: gif89a]]` 23 v): a frame with the User Input
/// Flag and no delay waits however long the clock runs, and a key moves
/// it on.
#[test]
fn a_gif_frame_waiting_for_input_moves_on_a_key() {
    let mut first = flat(10, 10, 1, 0);
    first.gce = Some((1, 0, true));
    let g = gif(10, 10, Some(0), &[first, flat(10, 10, 2, 10)]);
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &g), 4097);
        t.animate_images(gif_at(0), ImageMotion::Play);
        assert_eq!(
            t.animate_images(gif_at(60_000), ImageMotion::Play),
            Animated::Same
        );
        assert_eq!(t.next_image_frame(gif_at(60_000)), None, "nothing due");
        assert_eq!(placed(t).3[0], GIF_COLOURS[1], "waiting");
        let a = KeyCode::Char(Codepoint::from_char('a'));
        t.key(KeyEvent::press(a, Modifiers::NONE));
        assert_eq!(t.next_image_frame(gif_at(60_001)), Some(gif_at(60_001)));
        assert_eq!(
            t.animate_images(gif_at(60_001), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(placed(t).3[0], GIF_COLOURS[2], "a key moved it on");
    });
}

/// **A button pressed is user input too**, "Mouse Button Click" among
/// GIF89a's examples: with mouse tracking on, a press the core reports
/// moves a waiting frame on. A click the terminal takes itself is
/// `a_gif_frame_waiting_for_input_moves_on_any_press`.
#[test]
fn a_gif_frame_waiting_for_input_moves_on_a_press() {
    let mut first = flat(10, 10, 1, 0);
    first.gce = Some((1, 0, true));
    let g = gif(10, 10, Some(0), &[first, flat(10, 10, 2, 10)]);
    with(1 << 16, |t| {
        feed(t, b"\x1b[?1000h", 4097);
        feed(t, &file("inline=1", &g), 4097);
        t.animate_images(gif_at(0), ImageMotion::Play);
        assert_eq!(
            t.animate_images(gif_at(9_000), ImageMotion::Play),
            Animated::Same
        );
        let release = MouseEvent {
            button: MouseButton::Left,
            mods: MouseMods::NONE,
            kind: MouseKind::Release,
            col: Col::ZERO,
            row: Row::ZERO,
            px: None,
        };
        t.mouse_report(release);
        assert_eq!(
            t.animate_images(gif_at(9_001), ImageMotion::Play),
            Animated::Same,
            "a release is not a press"
        );
        t.mouse_report(MouseEvent {
            kind: MouseKind::Press,
            ..release
        });
        assert_eq!(
            t.animate_images(gif_at(9_002), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(placed(t).3[0], GIF_COLOURS[2], "the press moved it on");
    });
}

/// **A press the terminal takes itself moves a waiting frame on too** --
/// a selection with tracking off, the scrollbar, a link -- as the
/// embedder tells the core of every press (`Term::pointer_pressed`),
/// and nothing is sent: a frame waits for the user, not for a report.
#[test]
fn a_gif_frame_waiting_for_input_moves_on_any_press() {
    let mut first = flat(10, 10, 1, 0);
    first.gce = Some((1, 0, true));
    let g = gif(10, 10, Some(0), &[first, flat(10, 10, 2, 10)]);
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &g), 4097);
        t.animate_images(gif_at(0), ImageMotion::Play);
        assert_eq!(
            t.animate_images(gif_at(9_000), ImageMotion::Play),
            Animated::Same
        );
        assert_eq!(t.next_image_frame(gif_at(9_000)), None, "nothing due");
        t.pointer_pressed();
        assert!(t.reply().is_empty(), "nothing sent");
        assert_eq!(t.next_image_frame(gif_at(9_001)), Some(gif_at(9_001)));
        assert_eq!(
            t.animate_images(gif_at(9_001), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(placed(t).3[0], GIF_COLOURS[2], "the press moved it on");
    });
}

/// **Text over an animated GIF clears its cell in every frame**, so the
/// next frame shows none of the picture there either.
#[test]
fn text_over_an_animated_gif_clears_every_frame() {
    let g = gif(20, 20, Some(0), &[flat(20, 20, 1, 10), flat(20, 20, 2, 10)]);
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &g), 4097);
        feed(t, b"\x1b[1;2HX", 4097);
        t.animate_images(gif_at(0), ImageMotion::Play);
        for (ms, frame) in [(0, 1), (100, 2)] {
            if ms > 0 {
                assert_eq!(
                    t.animate_images(gif_at(ms), ImageMotion::Play),
                    Animated::Changed
                );
            }
            let (_, _, _, got) = placed(t);
            for y in 0..20 {
                for x in 0..20 {
                    let want = match (10..20).contains(&x) && y < 20 {
                        true => 0,
                        false => GIF_COLOURS[frame],
                    };
                    assert_eq!(got[y * 20 + x], want, "({x}, {y}) in frame {frame}");
                }
            }
        }
    });
}

/// **An animated GIF drawn at another size keeps its frames and timing**,
/// each resampled as a still picture is.
#[test]
fn a_gif_at_another_size_keeps_its_frames() {
    let g = gif(10, 10, Some(0), &[flat(10, 10, 1, 10), flat(10, 10, 3, 10)]);
    with(1 << 16, |t| {
        feed(t, &file("inline=1;width=2", &g), 4097);
        let (_, w, h, got) = placed(t);
        assert_eq!((w, h), (20, 20));
        assert!(got.iter().all(|&p| p == GIF_COLOURS[1]));
        t.animate_images(gif_at(0), ImageMotion::Play);
        assert_eq!(
            t.animate_images(gif_at(100), ImageMotion::Play),
            Animated::Changed
        );
        assert!(
            placed(t).3.iter().all(|&p| p == GIF_COLOURS[3]),
            "the second frame"
        );
    });
}

/// **An animation the image memory cannot hold draws the placeholder**,
/// counted, as a picture too big for it does. Each frame is stored as the
/// rectangle in which it differs from the one before: eight 40 by 40
/// frames, each of one colour and the next of another, take 1600 words of
/// canvas, one for the frame it holds, one saying whether its rows have
/// moved, 800 of row map, one a frame for where its record starts, and a
/// record each of 7 words of head, a colour and 400 words of indexes --
/// 5674 words, where the arena holds 4096. The three such frames that
/// needed 4802 words stored whole, and were refused, take 3629 and play. Fails if a record were written past the gap the
/// picture claimed instead of refused, or a record's head took more words.
#[test]
fn a_gif_too_big_for_the_memory_is_refused_whole() {
    let colours = [1, 2, 3, 1, 2, 3, 1, 2].map(|i| flat(40, 40, i, 10));
    let g = gif(40, 40, Some(0), &colours);
    with(1 << 12, |t| {
        feed(t, &file("inline=1", &g), 4097);
        let (_, w, h, _) = placed(t);
        assert_eq!((w, h), (35, 36), "the placeholder");
        assert_eq!(t.iterm_counts().of(ItermKey::File), ONE);
        assert_eq!(t.images().images_refused, 1, "the arena's refusal");
    });
    let g = gif(40, 40, Some(0), &colours[..3]);
    with(1 << 12, |t| {
        feed(t, &file("inline=1", &g), 4097);
        let imgs = t.images();
        let (_, p) = imgs.placements().next().expect("placed");
        assert_eq!(imgs.stored_words(p.image), Some(3629));
        assert_eq!(imgs.image(p.image).unwrap().width.raw(), 40, "the picture");
        assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
    });
}

/// `baud-app`'s image memory, `host::IMAGE_BUDGET`: 95 MiB of words.
const APP_IMAGES: usize = 95 << 18;

/// README's logo film `name`, as recorded.
fn logo(name: &str) -> Vec<u8> {
    let path = format!("{}/../assets/readme/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// **Every frame the decoder makes of `g`**, as the store keeps pixels,
/// with its gap: the reference, the decoder being held to ImageMagick's
/// own decode by `imagemagicks_streams_decode_to_its_reference`.
fn decoded(g: &[u8]) -> Vec<(Vec<u32>, u32)> {
    let gif = baud_domain::gif::Gif::parse(g).unwrap();
    let mut scratch = vec![0u8; gif.scratch_len()];
    let mut out = Vec::new();
    gif.decode(&mut scratch, |f| {
        let px = f
            .rgba
            .chunks(4)
            .map(|c| u32::from_be_bytes([c[3], c[0], c[1], c[2]]))
            .collect();
        out.push((px, f.gap_ms));
        true
    })
    .unwrap();
    out
}

/// **README's logo film plays in the app's image memory**: 320 frames of
/// 840 by 580, which stored whole needed 594.7 MiB and were refused, are
/// stored in 32 MiB or less, and on the clock every frame's canvas is the
/// decoder's whole frame, the loop's wrap back to the first included, one
/// compose a frame. Fails if the table counted the colours of the box
/// round the changed pixels, not the changed pixels' -- 319 of the 320
/// frames then past 255 colours and ARGB, 80.05 MiB measured, where the
/// register's about 81 MiB is every box as ARGB -- and if a record's
/// index named its colour off by one.
#[test]
fn the_readme_logo_plays_in_the_apps_image_memory() {
    let ws = WinSize {
        ws_row: 40,
        ws_col: 100,
    };
    for name in ["logo-dark.gif", "logo-light.gif"] {
        let g = logo(name);
        let want = decoded(&g);
        let n = want.len();
        assert_eq!(n, 320, "{name}: the premise, its frames");
        let mut owned = Owned::new(&ws).with_images(ImageBudget::of(APP_IMAGES));
        let mut t = Term::new(owned.storage(), &ws);
        t.set_cell_size(ImagePx::of(10), ImagePx::of(20));
        feed(&mut t, &file("inline=1", &g), 1 << 20);
        assert_eq!(
            t.iterm_counts().of(ItermKey::File),
            Messages::NONE,
            "{name}"
        );
        assert_eq!(t.images().images_refused, 0, "{name}: nothing refused");
        let id = t.images().placements().next().expect("placed").1.image;
        let v = t.images().image(id).unwrap();
        assert_eq!((v.width.raw(), v.height.raw()), (840, 580), "{name}: as is");
        let words = t.images().stored_words(id).unwrap();
        assert!(words * 4 <= 32 << 20, "{name}: {words} words stored");
        let shown = |t: &Term<'_>| -> Vec<u32> {
            let v = t.images().image(id).unwrap();
            v.pixels.iter().map(|p| p.bits()).collect()
        };
        assert!(shown(&t) == want[0].0, "{name}: the first frame");
        assert_eq!(
            t.animate_images(gif_at(0), ImageMotion::Play),
            Animated::Same
        );
        let mut ms = 0;
        for k in (1..n).chain([0]) {
            ms += u64::from(want[(k + n - 1) % n].1);
            let composed = t.images().frames_composed;
            assert_eq!(
                t.animate_images(gif_at(ms), ImageMotion::Play),
                Animated::Changed,
                "{name}: frame {k}"
            );
            assert_eq!(
                t.images().frames_composed,
                composed + 1,
                "{name}: one compose for frame {k}"
            );
            assert!(shown(&t) == want[k].0, "{name}: frame {k}'s pixels");
        }
    }
}

/// **A picture's frame is composed at the tick, never by bytes**: a
/// megabyte of text written below an animated GIF composes nothing, and
/// nor do the writes that reach its frames -- text over one of its cells,
/// an erase of another, and a scroll under side margins across it -- a
/// tick before the gap has passed composes nothing, the tick that moves
/// the frame composes it once and a second tick at that time none.
/// Fails if the frame were composed as bytes reached it: as the clock
/// moved it inside a read, or as an erase or a margin scroll changed its
/// frames.
#[test]
fn a_gif_frame_is_composed_at_the_tick_and_never_by_bytes() {
    let g = gif(40, 40, Some(0), &[flat(40, 40, 1, 10), flat(40, 40, 2, 10)]);
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &g), 4097);
        let composed = |t: &Term<'_>| t.images().frames_composed;
        let text = b"\x1b[24;1Hthe quick brown fox jumps over the lazy dog".repeat(22_000);
        assert!(text.len() > 1 << 20);
        feed(t, &text, 4096);
        assert_eq!(composed(t), 0, "bytes compose nothing");
        // Text on cell 1 of row 1, an erase of cell 0, both over the
        // picture, then margins at columns 3 to 10 across it and SU 1.
        let before = placed(t).3;
        feed(t, b"\x1b[1;2HX\x1b[1;1H\x1b[X", 4097);
        assert_ne!(placed(t).3, before, "the premise: the writes cleared cells");
        assert_eq!(composed(t), 0, "text and an erase over it compose nothing");
        let before = placed(t).3;
        feed(t, b"\x1b[?69h\x1b[3;10s\x1b[1;10r\x1b[S", 4097);
        assert_ne!(placed(t).3, before, "the premise: the scroll moved pixels");
        assert_eq!(composed(t), 0, "a margin scroll across it composes nothing");
        assert_eq!(
            t.animate_images(gif_at(0), ImageMotion::Play),
            Animated::Same
        );
        assert_eq!(
            t.animate_images(gif_at(50), ImageMotion::Play),
            Animated::Same
        );
        assert_eq!(composed(t), 0, "a tick before the gap composes nothing");
        assert_eq!(
            t.animate_images(gif_at(100), ImageMotion::Play),
            Animated::Changed
        );
        assert_eq!(composed(t), 1, "the tick that moves it composes once");
        // Left of the margin and below the cells written: untouched.
        assert_eq!(placed(t).3[39 * 40], GIF_COLOURS[2]);
        assert_eq!(
            t.animate_images(gif_at(100), ImageMotion::Play),
            Animated::Same
        );
        assert_eq!(composed(t), 1, "nothing moved since");
    });
}

/// The four frames of a 10 by 10 GIF whose later frames are rectangles:
/// white; a red square at (2, 2) put back by disposal 3; a green one at
/// (5, 5) put back to the background by disposal 2; a red pixel at (0, 0).
fn rectangles() -> Vec<u8> {
    let at = |rect: [u16; 4], i: u8, disposal: u8| Graphic {
        rect,
        px: vec![i; usize::from(rect[2]) * usize::from(rect[3])],
        gce: Some((disposal, 10, false)),
    };
    gif(
        10,
        10,
        Some(0),
        &[
            flat(10, 10, 1, 10),
            at([2, 2, 4, 4], 2, 3),
            at([5, 5, 3, 3], 3, 2),
            at([0, 0, 1, 1], 2, 1),
        ],
    )
}

/// **An animated GIF of rectangles drawn at another size**: each frame
/// on the clock is the decoder's frame resampled as every drawing path
/// samples, the source pixel under each drawn pixel's centre -- the
/// disposals' restored pixels included -- and its timing kept.
#[test]
fn a_gif_of_rectangles_at_another_size_resamples_each_frame() {
    let g = rectangles();
    let want = decoded(&g);
    assert_eq!(want.len(), 4, "the premise");
    with(1 << 16, |t| {
        feed(t, &file("inline=1;width=2", &g), 4097);
        let (_, w, h, _) = placed(t);
        assert_eq!((w, h), (20, 20));
        t.animate_images(gif_at(0), ImageMotion::Play);
        for k in 0..6 {
            if k > 0 {
                assert_eq!(
                    t.animate_images(gif_at(100 * k as u64), ImageMotion::Play),
                    Animated::Changed,
                    "tick {k}"
                );
            }
            let f = &want[k % 4].0;
            let near = |d: usize| {
                usize::try_from(ImageDraw::nearest(u32::try_from(d).unwrap(), 20, 0, 10)).unwrap()
            };
            let resampled: Vec<u32> = (0..20 * 20)
                .map(|i| f[near(i / 20) * 10 + near(i % 20)])
                .collect();
            assert!(placed(t).3 == resampled, "frame {}, tick {k}", k % 4);
        }
    });
}

/// **A scroll under side margins moves every frame of a GIF across them**,
/// in an arena of `arena` words: an animation is not split
/// (`ImageStore::split_across`), so its pixels over the cells inside the
/// margins move within it in each frame, the rows left behind cleared. A
/// 20 by 40 GIF at cell 0 covers cells 0 and 1 of rows 1 and 2; margins
/// at columns 2 to 10 take its right half, and SU 1 moves that half's rows
/// up 20 pixels in the frame shown and in every frame after. Text written
/// on the moved half then clears its cell in every frame too. The GIF has
/// no global table and its first frame leaves its last ten rows clear,
/// which its second paints, so the wrap back to the first frame draws
/// that frame's clear ground under the moved rows. Fails if a record over
/// no frame, drawn through the moved rows, kept the canvas under it
/// instead of filling it with its ground (`Shape::apply_moved`).
fn a_margin_scroll_moves_every_frame(arena: usize) {
    let stripes: Vec<u8> = (0..20 * 30).map(|i| [1, 2, 3][i / 20 / 7 % 3]).collect();
    let first = Graphic {
        rect: [0, 0, 20, 30],
        px: stripes,
        gce: Some((1, 10, false)),
    };
    let lower = Graphic {
        rect: [0, 20, 20, 20],
        px: vec![2; 400],
        gce: Some((1, 10, false)),
    };
    let g = gif_tables(20, 40, Some(0), &[first, lower], Tables::Local);
    let want = decoded(&g);
    assert_eq!(want[0].0[39 * 20 + 15], 0, "the premise: clear in frame 0");
    assert_ne!(
        want[1].0[39 * 20 + 15],
        0,
        "the premise: painted in frame 1"
    );
    let moved = |k: usize, erased: bool| -> Vec<u32> {
        let f = &want[k].0;
        (0..20 * 40)
            .map(|i| {
                let (x, y) = (i % 20, i / 20);
                match (x >= 10, y + 20 < 40) {
                    (false, _) => f[i],
                    (true, _) if erased && y < 20 => 0,
                    (true, true) => f[(y + 20) * 20 + x],
                    (true, false) => 0,
                }
            })
            .collect()
    };
    with(arena, |t| {
        feed(t, &file("inline=1", &g), 4097);
        let id = t.images().placements().next().expect("placed").1.image;
        let words = t.images().stored_words(id).unwrap();
        assert!(
            arena >= 1 << 16 || 2 * words > arena,
            "{arena}: the premise, no room for a copy of {words} words"
        );
        feed(t, b"\x1b[?69h\x1b[2;10s\x1b[1;10r\x1b[S", 4097);
        assert_eq!(t.images().images_refused, 0, "{arena}: nothing refused");
        t.animate_images(gif_at(0), ImageMotion::Play);
        let check = |t: &Term<'_>, k: usize, erased: bool, what: &str| {
            let (_, w, h, got) = placed(t);
            assert_eq!((w, h), (20, 40), "{arena}: {what}");
            assert!(got == moved(k, erased), "{arena}: {what}");
        };
        check(t, 0, false, "frame 0 moved");
        assert_eq!(
            t.animate_images(gif_at(100), ImageMotion::Play),
            Animated::Changed
        );
        check(t, 1, false, "frame 1 moved");
        assert_eq!(
            t.animate_images(gif_at(200), ImageMotion::Play),
            Animated::Changed
        );
        check(
            t,
            0,
            false,
            "frame 0 again, its clear ground under the moved rows",
        );
        feed(t, b"\x1b[1;2HX", 4097);
        check(t, 0, true, "frame 0 moved, then its cell cleared");
        for (ms, k) in [(300, 1), (400, 0)] {
            assert_eq!(
                t.animate_images(gif_at(ms), ImageMotion::Play),
                Animated::Changed
            );
            check(t, k, true, &format!("frame {k} at {ms} ms"));
        }
        assert_eq!(
            t.images().stored_words(id),
            Some(words),
            "{arena}: moved in its own span"
        );
    });
}

#[test]
fn a_margin_scroll_moves_every_frame_of_a_gif() {
    a_margin_scroll_moves_every_frame(1 << 16);
}

/// **The move needs no room beside the picture**: the same GIF and scroll
/// in an arena of 2048 words, where the picture takes 1472 and a moved
/// copy of it could not be stored. Fails under a move that copies the
/// picture, which found no room here and removed the placement.
#[test]
fn a_margin_scroll_moves_a_gif_with_no_room_beside_it() {
    a_margin_scroll_moves_every_frame(1 << 11);
}

/// A 4 by 4 GIF of index 1 with a Plain Text Extension after it: one
/// 4 by 4 cell, foreground index 2, background index 0, its text "hi".
fn gif_with_text() -> Vec<u8> {
    let mut g = gif(4, 4, None, &[flat(4, 4, 1, 0)]);
    let at = g.len() - 1;
    let text = [
        0x21, 0x01, 12, 0, 0, 0, 0, 4, 0, 4, 0, 4, 4, 2, 0, 2, b'h', b'i', 0,
    ];
    g.splice(at..at, text);
    g
}

/// A font that sets every pixel of its cell.
fn solid(_: char, w: ImagePx, h: ImagePx, set: &mut dyn FnMut(ImagePx, ImagePx)) {
    for y in 0..h.raw() {
        for x in 0..w.raw() {
            set(ImagePx::of(x), ImagePx::of(y));
        }
    }
}

/// **A Plain Text Extension is drawn with the font `set_regis_font`
/// lends**, over the image before it, and nothing is counted. Fails if the
/// stored frame shows the image where the text is.
#[test]
fn a_gif_plain_text_extension_is_drawn_with_the_lent_font() {
    let g = gif_with_text();
    with(1 << 16, |t| {
        t.set_regis_font(solid);
        feed(t, &file("inline=1", &g), 4097);
        let (_, w, h, got) = placed(t);
        assert_eq!((w, h), (4, 4));
        assert!(
            got.iter().all(|&p| p == GIF_COLOURS[2]),
            "the text's foreground over the image"
        );
        assert_eq!(t.iterm_counts().picture_parts_skipped, Messages::NONE);
        assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
    });
}

/// **With no font lent a Plain Text Extension is not drawn, and is
/// counted** in `picture_parts_skipped`; the picture round it draws.
#[test]
fn a_gif_plain_text_extension_with_no_font_is_counted() {
    let g = gif_with_text();
    with(1 << 16, |t| {
        feed(t, &file("inline=1", &g), 4097);
        let (_, w, h, got) = placed(t);
        assert_eq!((w, h), (4, 4));
        assert!(got.iter().all(|&p| p == GIF_COLOURS[1]), "the image alone");
        assert_eq!(t.iterm_counts().picture_parts_skipped, ONE);
        assert_eq!(t.iterm_counts().of(ItermKey::File), Messages::NONE);
    });
}

/// **"N: N character cells"**, the height from the aspect: six cells wide
/// is 60 pixels, 80 tall, the picture resampled to them as every drawing
/// path samples, the pixel under each drawn pixel's centre.
#[test]
fn a_width_in_cells_resamples_with_the_aspect() {
    let (png, px) = picture(30, 40, 9);
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &file("inline=1;width=6", &png), size);
            let (_, w, h, got) = placed(t);
            assert_eq!((w, h), (60, 80), "reads of {size}");
            // Twice the size: each source pixel drawn two by two.
            for (i, &p) in got.iter().enumerate() {
                let (x, y) = (i % 60, i / 60);
                assert_eq!(p, px[(y / 2) * 30 + x / 2], "({x}, {y}), reads of {size}");
            }
            assert_eq!(t.cursor(), (Row::from_index(3), Col::at_index(6)));
        });
    }
}

/// **The multipart form is the same picture**: `MultipartFile`, the base64
/// in `FilePart`s, `FileEnd`, with other output between the parts -- parts
/// of 1024 characters, and parts of 1023 and 1001 that cut the base64's
/// quanta, which iTerm2 joins before it decodes them; a `FilePart` and a
/// `FileEnd` with none open are counted.
#[test]
fn a_multipart_file_is_the_same_picture() {
    let (png, px) = picture(30, 40, 11);
    let coded = b64(&png);
    let parts = |cuts: &[usize]| {
        let mut bytes = b"\x1b]1337;MultipartFile=inline=1\x07".to_vec();
        let mut at = 0;
        let mut k = 0;
        while at < coded.len() {
            let n = cuts[k % cuts.len()].min(coded.len() - at);
            bytes.extend(b"\x1b]1337;FilePart=");
            bytes.extend(&coded[at..at + n]);
            bytes.extend(b"\x07\x1b[m");
            at += n;
            k += 1;
        }
        bytes.extend(b"\x1b]1337;FileEnd\x07");
        bytes
    };
    for bytes in [parts(&[1024]), parts(&[1023, 1001])] {
        for size in SIZES {
            with(1 << 16, |t| {
                feed(t, &bytes, size);
                let (_, w, h, got) = placed(t);
                assert_eq!((w, h), (30, 40), "reads of {size}");
                assert!(got == px, "the pixels, reads of {size}");
            });
        }
    }
    let bytes = parts(&[1024]);
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &bytes, size);
            let (_, w, h, got) = placed(t);
            assert_eq!((w, h), (30, 40), "reads of {size}");
            assert!(got == px, "the pixels, reads of {size}");
            feed(t, b"\x1b]1337;FilePart=AAAA\x07\x1b]1337;FileEnd\x07", size);
            assert_eq!(t.iterm_counts().of(ItermKey::FilePart), ONE);
            assert_eq!(t.iterm_counts().of(ItermKey::FileEnd), ONE);
        });
    }
}

/// **`RIS` abandons a file in parts**: a terminal made operational has
/// none open (`[[source: ecma48]]` 8.3.105), so the parts after it and its
/// `FileEnd` are counted, none open, and nothing is placed; `DECSTR` keeps
/// it open, and the picture is shown.
#[test]
fn ris_abandons_a_file_in_parts_and_decstr_keeps_it() {
    let (png, px) = picture(30, 40, 11);
    let coded = b64(&png);
    let (head, tail) = coded.split_at(coded.len() / 2 / 4 * 4);
    let file = |reset: &[u8]| {
        let mut bytes = b"\x1b]1337;MultipartFile=inline=1\x07".to_vec();
        for (n, part) in [head, tail].into_iter().enumerate() {
            if n == 1 {
                bytes.extend(reset);
            }
            bytes.extend(b"\x1b]1337;FilePart=");
            bytes.extend(part);
            bytes.extend(b"\x07");
        }
        bytes.extend(b"\x1b]1337;FileEnd\x07");
        bytes
    };
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &file(b"\x1bc"), size);
            assert_eq!(t.images().placements().count(), 0, "reads of {size}");
            assert_eq!(t.iterm_counts().of(ItermKey::FilePart), ONE);
            assert_eq!(t.iterm_counts().of(ItermKey::FileEnd), ONE);
        });
        with(1 << 16, |t| {
            feed(t, &file(b"\x1b[!p"), size);
            let (_, w, h, got) = placed(t);
            assert_eq!((w, h), (30, 40), "DECSTR, reads of {size}");
            assert!(got == px, "the pixels, reads of {size}");
        });
    }
}

/// **"Otherwise, it will be downloaded"**: refused, counted, nothing placed
/// and the cursor where it was; so is a file past the scratch, whole.
#[test]
fn a_download_and_a_file_past_the_scratch_are_refused() {
    let (png, _) = picture(30, 40, 13);
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &file("name=eA==", &png), size);
            assert_eq!(t.images().placements().count(), 0, "reads of {size}");
            assert_eq!(t.cursor(), (Row::from_index(0), Col::at_index(0)));
            assert_eq!(t.iterm_counts().of(ItermKey::File), ONE, "reads of {size}");
        });
        // An arena of 512 pixels lends 4096 bytes of scratch.
        with(512, |t| {
            feed(t, &file("inline=1", &png), size);
            assert_eq!(t.images().placements().count(), 0, "reads of {size}");
            assert_eq!(t.iterm_counts().of(ItermKey::File), ONE, "reads of {size}");
        });
    }
}

/// **What does not decode draws the broken-image placeholder**, iTerm2's
/// 35 by 36 pixels for `auto`, a frame and its diagonals in the foreground,
/// clear inside, and the cursor past it; counted.
#[test]
fn what_does_not_decode_draws_the_placeholder() {
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, &file("inline=1", b"not a picture at all"), size);
            let (_, w, h, got) = placed(t);
            assert_eq!((w, h), (35, 36), "reads of {size}");
            let white = 0xffff_ffff;
            assert_eq!(got[0], white, "the frame's corner");
            assert_eq!(got[35 * 36 - 1], white, "the opposite corner");
            assert_eq!(got[18 * 35 + 17], white, "the diagonals cross");
            assert_eq!(got[5 * 35 + 20], 0, "clear inside");
            assert_eq!(t.cursor(), (Row::from_index(1), Col::at_index(4)));
            assert_eq!(t.iterm_counts().of(ItermKey::File), ONE, "reads of {size}");
        });
    }
}

/// **The picture's cells take no text, and text written over one cell
/// clears that cell alone**, as iTerm2's image characters are written and
/// replaced.
#[test]
fn the_cells_are_the_pictures_and_text_clears_one() {
    let (png, px) = picture(30, 40, 17);
    for size in SIZES {
        with(1 << 16, |t| {
            feed(t, b"abcdefgh\r", size);
            feed(t, &file("inline=1", &png), size);
            let row0: Vec<Content> = t.viewport().row(Row::from_index(0))[..4]
                .iter()
                .map(|c| c.content)
                .collect();
            assert_eq!(row0[..3], [Content::EMPTY; 3], "erased, reads of {size}");
            assert_eq!(
                row0[3],
                Content::codepoint(Codepoint::from_char('d')),
                "past the picture"
            );
            // `X` over the second cell of the first row.
            feed(t, b"\x1b[1;2HX", size);
            let (_, _, _, got) = placed(t);
            for y in 0..40 {
                for x in 0..30 {
                    let want = match (10..20).contains(&x) && y < 20 {
                        true => 0,
                        false => px[y * 30 + x],
                    };
                    assert_eq!(got[y * 30 + x], want, "({x}, {y}), reads of {size}");
                }
            }
        });
    }
}

thread_local! {
    /// The bytes the page-release double was handed on this thread.
    static HANDED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn count(span: &mut [u8]) -> baud_domain::PagesBack {
    HANDED.with(|h| h.set(h.get() + span.len()));
    baud_domain::PagesBack::Released
}

/// The bytes handed to the release while `f` runs.
fn handed(t: &mut Term<'_>, f: impl FnOnce(&mut Term<'_>)) -> usize {
    HANDED.with(|h| h.set(0));
    t.set_page_release(baud_domain::PageRelease::new(64, count).expect("a power of two"));
    f(t);
    HANDED.with(std::cell::Cell::get)
}

/// **The scratch a file wrote goes back, however it ends**, as kitty's
/// PNGs give it back: a picture shown hands back its bytes and the
/// scanlines decoded after them, a file past the scratch the whole of it,
/// and a file abandoned by a kitty command what it had written.
#[test]
fn the_scratch_a_file_wrote_goes_back() {
    let (png, _) = picture(30, 40, 23);
    let coded = b64(&png);
    with(1 << 16, |t| {
        let n = handed(t, |t| feed(t, &file("inline=1", &png), 4096));
        // The PNG, and 40 rows of 1 + 120 bytes of scanlines after it.
        assert!(
            n + 128 >= png.len() + 40 * 121,
            "{n} of {}",
            png.len() + 40 * 121
        );
    });
    // An arena of 512 pixels lends 4096 bytes of scratch, filled but for
    // the last decoded run that did not fit, at most 768 octets.
    with(512, |t| {
        let n = handed(t, |t| feed(t, &file("inline=1", &png), 4096));
        assert!(n + 768 + 128 >= 4096, "{n} of 4096");
    });
    with(1 << 16, |t| {
        let n = handed(t, |t| {
            feed(
                t,
                b"\x1b]1337;MultipartFile=inline=1\x07\x1b]1337;FilePart=",
                4096,
            );
            feed(t, &coded[..2000], 4096);
            feed(t, b"\x07\x1b_Ga=q,i=9,s=1,v=1,f=24;AAAA\x1b\\", 4096);
        });
        assert!(n + 128 >= 1500, "{n} of the 1500 octets written");
    });
}

/// **One scratch for two streams**: a file abandons a kitty PNG still
/// arriving in chunks, counted cancelled, and a kitty command abandons a
/// file in parts, counted under its key.
#[test]
fn a_file_and_a_kitty_png_take_the_scratch_in_turn() {
    let (png, _) = picture(30, 40, 19);
    let coded = b64(&png);
    for size in SIZES {
        with(1 << 16, |t| {
            let mut first = b"\x1b_Gf=100,m=1;".to_vec();
            first.extend(&coded[..400]);
            first.extend(b"\x1b\\");
            feed(t, &first, size);
            let before: KittyCounts = t.kitty();
            feed(t, &file("inline=1", &png), size);
            assert_eq!(t.kitty().cancelled, before.cancelled + 1, "reads of {size}");
            assert_eq!(t.images().placements().count(), 1, "the file's picture");
            feed(
                t,
                b"\x1b]1337;MultipartFile=inline=1\x07\x1b]1337;FilePart=",
                size,
            );
            feed(t, &coded[..400], size);
            feed(t, b"\x07\x1b_Ga=q,i=9,s=1,v=1,f=24;AAAA\x1b\\", size);
            assert_eq!(
                t.iterm_counts().of(ItermKey::MultipartFile),
                ONE,
                "reads of {size}"
            );
            feed(t, b"\x1b]1337;FileEnd\x07", size);
            assert_eq!(t.iterm_counts().of(ItermKey::FileEnd), ONE, "none open");
        });
    }
}
